//! The browser suites (Q24). `playground`: the release `ardana` from `cargo xtask build`, copied alone into
//! `tmp/e2e/playground-binary` and serving decider-2b while the playground's `dist/` is moved away (plus one server
//! on an empty registry, for `first_run`, `browser_stop` and `browser_recover`, one more for `browser_run`,
//! `insecure_origin` and `browser_resume`, one over an empty Hub cache for `browser_pull`, and one more empty one whose
//! registry `pull_while_open` pulls into with `ardana pull`), driven by the Playwright cases in
//! `e2e/playground` on the installed Chrome; plus the placeholder build of `ardana-server`. `public`: the release
//! `ardana serve --public` on an empty home, its HTTP surface probed with curl and its playground driven by the
//! `@public` Playwright case. `design`: the /impeccable context of the playground, `impeccable detect` on its six URL
//! states and its two in-tab states (frozen by the `@design` Playwright test) at 1280x800 and 390x844, and the finish
//! (critique record, audit, clean scans, hook on).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::e2e::read_json;
use crate::playground;
use crate::sandbox::{Sandbox, cargo};
use crate::serve::Server;

/// The Playwright cases of the playground suite; each saves at least one screenshot per viewport under
/// `tmp/screens/<case>/`.
pub const CASES: &[&str] = &[
    "embedded_binary",
    "answers_match_api",
    "picker_raw_errors",
    "builder_sync",
    "state_modes",
    "jev_share_links",
    "share_roundtrip",
    "presets",
    "snippets",
    "stale_and_restore",
    "run_command",
    "pull_while_open",
    "first_run",
    "browser_run",
    "insecure_origin",
    "browser_stop",
    "browser_recover",
    "browser_resume",
    "browser_pull",
    "banner_focus",
    "banner_reflow",
    "text_reflow",
    "term_tooltips",
    "skip_link",
    "share_popover",
    "announcements",
    "run_focus",
    "aria_relationships",
    "term_stops",
    "page_focus",
    "touch_targets",
    "current_row",
    "page_weight",
];
/// The Playwright cases of the public suite, screenshots alike.
pub const PUBLIC_CASES: &[&str] = &["public_playground"];
/// Playwright project name and viewport, as `playwright.config.ts` and `impeccable detect` use them.
pub const VIEWPORTS: &[(&str, u32, u32)] = &[("desktop", 1280, 800), ("mobile", 390, 844)];

/// The request the loaded and results states come from.
const TICKET: &str = "tests/fixtures/requests/ticket.json";
const PLAYGROUND_DIR: &str = "e2e/playground";
/// The Playwright tag of the design suite's tests, which the playground suite leaves out.
const DESIGN_TAG: &str = "@design";
/// The Playwright tag of the public suite's tests, which the playground suite leaves out.
const PUBLIC_TAG: &str = "@public";
/// The in-tab states the `@design` test freezes into `<state>-<project>.html` for `impeccable detect`.
const FROZEN_STATES: &[&str] = &["browser-download", "browser-results"];
const SURFACE: &str = "crates/ardana-playground";
/// The six blocks of the surface brief's direction contract.
const CONTRACT_BLOCKS: &[&str] = &[
    "THESIS",
    "OWN-WORLD",
    "STORY",
    "FIRST VIEWPORT",
    "FORM",
    "FINISH",
];

pub fn playground(sandbox: &Sandbox) -> Result<()> {
    let ardana = playground::build(sandbox)?;
    placeholder_without_dist(sandbox)?;

    let dir = sandbox.tmp().join("e2e/playground-binary");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join("ardana");
    std::fs::copy(&ardana, &copy).with_context(|| format!("copying {}", ardana.display()))?;
    println!(
        "e2e playground: embedded_binary serves from {}",
        copy.display()
    );

    clear_screens(sandbox, CASES)?;
    let status = {
        let _moved = MovedAway::new(
            &sandbox.repo_root().join("crates/ardana-playground/dist"),
            &sandbox.tmp().join("e2e/playground-dist-moved"),
        )?;
        let server = Server::start(sandbox, &copy, "playground", None)?;
        // `first_run`, `browser_stop` and `browser_recover` start on an empty registry; their runs in the tab leave it
        // empty for the next viewport.
        let empty = Server::start_empty(sandbox, &copy, "playground-empty")?;
        // `browser_run` and `browser_resume` compare the tab with decider-0.8b on the server, which pulls and loads it:
        // on a server of their own (which `insecure_origin` reaches under another host name, and `browser_resume`
        // through a proxy that drops its connections), the other cases keep a server holding decider-2b alone.
        let browser = Server::start_empty(sandbox, &copy, "playground-browser")?;
        // `browser_pull` needs a server that holds no browser variant: its own empty Hub cache, every pull failing.
        let uncached = Server::start_uncached(sandbox, &copy, "playground-uncached")?;
        // `pull_while_open` pulls a model into a registry with `ardana pull` while the page is open: a server of its own,
        // so the other servers' lists stay as their cases expect.
        let pull = Server::start_empty(sandbox, &copy, "playground-pull")?;
        let status = playwright(sandbox)
            .args([
                "test",
                "--grep-invert",
                &format!("{DESIGN_TAG}|{PUBLIC_TAG}"),
            ])
            .env("ARDANA_BASE_URL", &server.url)
            .env("ARDANA_EMPTY_URL", &empty.url)
            .env("ARDANA_BROWSER_URL", &browser.url)
            .env("ARDANA_UNCACHED_URL", &uncached.url)
            .env("ARDANA_PULL_URL", &pull.url)
            .env("ARDANA_PULL_HOME", &pull.home)
            // `snippets` runs the ardana commands it shows with this binary, and `pull_while_open` its pull.
            .env("ARDANA_BIN", &copy)
            .env("ARDANA_REPO_ROOT", sandbox.repo_root())
            .status()
            .context("running playwright test; run `cargo xtask fetch`")?;
        server.stop()?;
        empty.stop()?;
        browser.stop()?;
        uncached.stop()?;
        pull.stop()?;
        status
    };
    if !status.success() {
        bail!("playwright test failed ({status}); report in tmp/playwright/report");
    }
    let mut missing = screenshots(sandbox, CASES);
    let out = sandbox.tmp().join("playwright");
    for file in [
        "results/.last-run.json",
        "report/index.html",
        "results.json",
    ] {
        if !out.join(file).is_file() {
            missing.push(format!("{} is missing", out.join(file).display()));
        }
    }
    if !missing.is_empty() {
        bail!("screenshots: {}", missing.join("; "));
    }
    println!(
        "e2e playground: screenshots of {} cases at 1280 and 390 wide in {}",
        CASES.len(),
        sandbox.tmp().join("screens").display()
    );
    Ok(())
}

/// Empties each case's `tmp/screens/<case>/`.
fn clear_screens(sandbox: &Sandbox, cases: &[&str]) -> Result<()> {
    for case in cases {
        let dir = sandbox.tmp().join("screens").join(case);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
    }
    Ok(())
}

/// The suite's Playwright, run from `e2e/playground`.
fn playwright(sandbox: &Sandbox) -> std::process::Command {
    let dir = sandbox.repo_root().join(PLAYGROUND_DIR);
    let mut command = sandbox.command(dir.join("node_modules/.bin/playwright"));
    command.current_dir(dir);
    command
}

/// R6.7: what is missing of the 1280- and 390-wide screenshots (`<project>.png` or `<state>-<project>.png`) every case
/// must leave.
fn screenshots(sandbox: &Sandbox, cases: &[&str]) -> Vec<String> {
    let mut missing = Vec::new();
    for case in cases {
        let dir = sandbox.tmp().join("screens").join(case);
        let files: Vec<String> = std::fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        for (project, width, _) in VIEWPORTS {
            let shots: Vec<&String> = files
                .iter()
                .filter(|name| {
                    name.strip_suffix(&format!("{project}.png"))
                        .is_some_and(|state| state.is_empty() || state.ends_with('-'))
                })
                .collect();
            if shots.is_empty() {
                missing.push(format!("{} has no {project} screenshot", dir.display()));
            }
            for name in shots {
                let path = dir.join(name);
                match png_width(&path) {
                    Ok(w) if w == *width => {}
                    Ok(w) => {
                        missing.push(format!("{} is {w} px wide, not {width}", path.display()))
                    }
                    Err(err) => missing.push(format!("{}: {err:#}", path.display())),
                }
            }
        }
    }
    missing
}

/// The width of a PNG, from its IHDR chunk.
fn png_width(path: &Path) -> Result<u32> {
    let bytes = std::fs::read(path).context("no screenshot")?;
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        bail!("not a PNG");
    }
    Ok(u32::from_be_bytes([
        bytes[16], bytes[17], bytes[18], bytes[19],
    ]))
}

/// R6.4: `ardana-server` built in its own target dir with the playground dist path at an empty directory builds with
/// a warning, and serves the placeholder at `/` (its `playground` test, run against that build).
fn placeholder_without_dist(sandbox: &Sandbox) -> Result<()> {
    let dir = sandbox.tmp().join("e2e/placeholder");
    let empty = dir.join("empty-dist");
    if empty.exists() {
        std::fs::remove_dir_all(&empty)?;
    }
    std::fs::create_dir_all(&empty)?;
    let target = dir.join("target");
    let cargo_step = |args: &[&str]| {
        sandbox
            .command(cargo())
            .current_dir(sandbox.repo_root())
            .args(args)
            .arg("--target-dir")
            .arg(&target)
            .env("ARDANA_PLAYGROUND_DIST", &empty)
            .output()
            .with_context(|| format!("running cargo {}", args.join(" ")))
    };
    let build = cargo_step(&["build", "--locked", "--package", "ardana-server"])?;
    let stderr = String::from_utf8_lossy(&build.stderr);
    if !build.status.success() {
        bail!(
            "placeholder_without_dist: the build failed ({}):\n{stderr}",
            build.status
        );
    }
    let warning = format!(
        "warning: ardana-server@{}: no playground at",
        env!("CARGO_PKG_VERSION")
    );
    let Some(line) = stderr.lines().find(|line| line.starts_with(&warning)) else {
        bail!("placeholder_without_dist: no `{warning}` warning in:\n{stderr}");
    };
    println!("e2e playground: placeholder_without_dist warns `{line}`");
    let test = cargo_step(&[
        "test",
        "--locked",
        "--package",
        "ardana-server",
        "--test",
        "playground",
    ])?;
    if !test.status.success() {
        bail!(
            "placeholder_without_dist: the placeholder is not served ({}):\n{}{}",
            test.status,
            String::from_utf8_lossy(&test.stdout),
            String::from_utf8_lossy(&test.stderr)
        );
    }
    println!("e2e playground: placeholder_without_dist serves the placeholder at /");
    Ok(())
}

/// Moves a directory away while it lives and back when dropped, so a server can only serve what it embedded.
struct MovedAway {
    from: PathBuf,
    to: PathBuf,
}

impl MovedAway {
    fn new(from: &Path, to: &Path) -> Result<MovedAway> {
        if to.exists() {
            std::fs::remove_dir_all(to)?;
        }
        std::fs::rename(from, to).with_context(|| format!("moving {} away", from.display()))?;
        Ok(MovedAway {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        })
    }
}

impl Drop for MovedAway {
    fn drop(&mut self) {
        if let Err(err) = std::fs::rename(&self.to, &self.from) {
            eprintln!(
                "error: could not move {} back to {}: {err}",
                self.to.display(),
                self.from.display()
            );
        }
    }
}

/// R3.1 to R3.3 and R3.5 on the release binary: `ardana serve --public` on an empty `ARDANA_HOME`, offline over
/// `tmp/hf`. Its HTTP surface is probed with curl, every path sent as written; the `@public` Playwright case drives its
/// playground at both viewports; a second public server, over an empty Hub cache, shows that a failed pull answers
/// without its reason; and both homes are still empty afterwards.
pub fn public(sandbox: &Sandbox) -> Result<()> {
    let ardana = playground::build(sandbox)?;
    clear_screens(sandbox, PUBLIC_CASES)?;

    let server = Server::start_public(sandbox, &ardana, "public", false)?;
    let mut problems = public_surface(sandbox, &server.url)?;
    let status = playwright(sandbox)
        .args([
            "test",
            "--grep",
            PUBLIC_TAG,
            "--reporter",
            "list",
            "--output",
        ])
        .arg(sandbox.tmp().join("playwright/public-results"))
        .env("ARDANA_BASE_URL", &server.url)
        .env("ARDANA_REPO_ROOT", sandbox.repo_root())
        .status()
        .context("running playwright test; run `cargo xtask fetch`")?;
    if !status.success() {
        problems.push(format!(
            "the {PUBLIC_TAG} Playwright case failed ({status})"
        ));
    }
    let home = server.home.clone();
    server.stop()?;
    problems.extend(left_in(&home));

    // Over an empty Hub cache the server holds no browser variant, and every browser pull fails: the answer says what
    // failed, never why.
    let bare = Server::start_public(sandbox, &ardana, "public-no-cache", true)?;
    let mut probes = Probes::new(sandbox, &bare.url);
    let listed = probes.send("GET", "/v1/models", None)?;
    probes.expect(
        listed.status == 200 && !listed.body.contains("x_browser_pulled"),
        || {
            format!(
                "GET /v1/models over an empty cache marks a browser variant held: {}",
                listed.body
            )
        },
    );
    let path = "/v1/browser/decider-0.8b/profile";
    let failed = probes.send("GET", path, None)?;
    let said = serde_json::from_str::<Value>(&failed.body).ok();
    probes.expect(
        failed.status == 500
            && said
                == Some(json!({"detail": {"error_type": "api_error", "message":
                    "the browser files of decider-0.8b are not available on this server; try again later"}})),
        || format!("GET {path} over an empty cache: {} {}", failed.status, failed.body),
    );
    probes.tells_no_path(path, &failed);
    let log = bare.log_text();
    probes.expect(log.contains(&format!("GET {path}: pulling")), || {
        format!("the log of the failed pull gives no reason:\n{log}")
    });
    problems.extend(probes.problems);
    let home = bare.home.clone();
    bare.stop()?;
    problems.extend(left_in(&home));
    problems.extend(screenshots(sandbox, PUBLIC_CASES));
    if !problems.is_empty() {
        bail!("e2e public failed:\n  {}", problems.join("\n  "));
    }
    println!(
        "e2e public: over an empty cache the failed pull answers 500 without its reason; both homes are empty; \
         screenshots of {} at 1280 and 390 wide",
        PUBLIC_CASES.join(", ")
    );
    Ok(())
}

/// What `dir`, an Ardana home, holds; a public server writes nothing there.
fn left_in(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| format!("a public server wrote {}", entry.path().display()))
            .collect(),
        Err(err) => vec![format!("reading {}: {err}", dir.display())],
    }
}

/// R3.1, R3.2: every problem with the HTTP surface of the public server at `url`.
fn public_surface(sandbox: &Sandbox, url: &str) -> Result<Vec<String>> {
    let mut probes = Probes::new(sandbox, url);

    // Every library model, none pulled and none the default; the browser variants with their sizes, and the browser
    // default, as `library.toml` has them.
    let library: toml::Table = toml::from_str(&std::fs::read_to_string(
        sandbox.repo_root().join(crate::onnx::LIBRARY),
    )?)?;
    let browser_default = library.get("browser_default").and_then(|v| v.as_str());
    let expected: Vec<(String, Option<i64>, bool)> = library
        .get("model")
        .and_then(|models| models.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let name = model.get("name")?.as_str()?;
            let size = model
                .get("browser")
                .and_then(|b| b.get("size"))
                .and_then(|s| s.as_integer());
            Some((name.to_string(), size, Some(name) == browser_default))
        })
        .collect();
    let listed = probes.send("GET", "/v1/models", None)?;
    let models: Value = serde_json::from_str(&listed.body).unwrap_or_default();
    let models = models["models"].as_array().cloned().unwrap_or_default();
    let got: Vec<(String, Option<i64>, bool)> = models
        .iter()
        .map(|m| {
            (
                m["name"].as_str().unwrap_or_default().to_string(),
                m["x_browser"].as_i64(),
                m["x_browser_default"] == true,
            )
        })
        .collect();
    probes.expect(listed.status == 200 && got == expected, || {
        format!(
            "GET /v1/models lists {got:?}, not {expected:?}: {}",
            listed.body
        )
    });
    probes.expect(
        models
            .iter()
            .all(|m| m["x_pulled"] == false && m.get("x_default").is_none()),
        || {
            format!(
                "GET /v1/models marks a model pulled or the default: {}",
                listed.body
            )
        },
    );
    // `tmp/hf` holds decider-0.8b's browser variant whole (the `@public` case runs it), so the list says a request for
    // its files pulls nothing.
    probes.expect(
        models
            .iter()
            .any(|m| m["name"] == "decider-0.8b" && m["x_browser_pulled"] == true),
        || {
            format!(
                "GET /v1/models does not mark decider-0.8b's browser variant held: {}",
                listed.body
            )
        },
    );
    let health = probes.send("GET", "/health", None)?;
    probes.expect(
        health.status == 200 && health.body == r#"{"status":"ok"}"#,
        || format!("GET /health: {} {}", health.status, health.body),
    );

    // Every decision is refused with TypeSafe's 403, whatever it names.
    let ticket = read_json(&sandbox.repo_root().join(TICKET))?;
    let named = |model: &str| {
        let mut request = ticket.clone();
        request["model"] = json!(model);
        request.to_string()
    };
    for (label, body) in [
        ("jev-latest", ticket.to_string()),
        ("decider-2b", named("decider-2b")),
        ("decider-0.8b", named("decider-0.8b")),
        ("not JSON", r#"{"state": "#.to_string()),
    ] {
        let reply = probes.send("POST", "/v1/systemone", Some(&body))?;
        let error: Value = serde_json::from_str(&reply.body).unwrap_or_default();
        probes.expect(
            reply.status == 403 && error["detail"]["error_type"] == "permission_error",
            || {
                format!(
                    "POST /v1/systemone ({label}): {} {}",
                    reply.status, reply.body
                )
            },
        );
        probes.tells_no_path(label, &reply);
    }

    // Traversal sent as written and percent-encoded, unknown names, other files: the API's 404.
    for path in [
        "/v1/browser/../../models.toml",
        "/v1/browser/../profile",
        "/v1/browser/decider-0.8b/../profile",
        "/v1/browser/decider-0.8b/..",
        "/v1/browser/decider-0.8b/../../../../etc/passwd",
        "/v1/browser/%2e%2e/profile",
        "/v1/browser/..%2F..%2Fmodels.toml/profile",
        "/v1/browser/decider-0.8b/%2e%2e%2f%2e%2e%2fmodels.toml",
        "/v1/browser/decider-0.8b/..%2Fdecider-0.8b-GGUF%2Fdecider_config.json",
        "/v1/browser/decider-0.8b/%2Fetc%2Fpasswd",
        "/v1/browser/decider-4b/profile",
        "/v1/browser/smollm3-3b/model.onnx",
        "/v1/browser/nope/tokenizer.json",
        "/v1/browser/DECIDER-0.8B/profile",
        "/v1/browser/decider-0.8b:q8_0/profile",
        "/v1/browser/decider-0.8b/README.md",
        "/v1/browser/decider-0.8b/decider_config.json",
        "/v1/browser/decider-0.8b/tokenizer_config.json",
        "/v1/browser/decider-0.8b/model.onnx.datax",
        "/v1/models/decider-2b",
        "/v1/nope",
    ] {
        let reply = probes.send("GET", path, None)?;
        probes.expect(
            reply.status == 404 && reply.body == r#"{"detail":"Not Found"}"#,
            || format!("GET {path}: {} {}", reply.status, reply.body),
        );
        probes.tells_no_path(path, &reply);
    }
    // Other methods, a CORS preflight among them: the API's 405.
    for (method, path) in [
        ("POST", "/v1/browser/decider-0.8b/model.onnx"),
        ("PUT", "/v1/browser/decider-0.8b/profile"),
        ("DELETE", "/v1/browser/decider-0.8b/tokenizer.json"),
        ("POST", "/v1/models"),
        ("OPTIONS", "/v1/models"),
        ("OPTIONS", "/v1/systemone"),
        ("GET", "/v1/systemone"),
    ] {
        let reply = probes.send(method, path, None)?;
        probes.expect(
            reply.status == 405 && reply.body == r#"{"detail":"Method Not Allowed"}"#,
            || format!("{method} {path}: {} {}", reply.status, reply.body),
        );
    }
    let reply = probes.send("POST", "/health", None)?;
    probes.expect(reply.status == 405, || {
        format!("POST /health: {}", reply.status)
    });
    probes.tells_no_path("POST /health", &reply);
    // Outside /v1, a traversal is the playground's own page: it is served from memory, never from a file.
    let page = probes.send("GET", "/", None)?;
    for path in ["/../../../etc/passwd", "/..%2F..%2F..%2Fetc%2Fpasswd"] {
        let reply = probes.send("GET", path, None)?;
        probes.expect(reply.status == 200 && reply.body == page.body, || {
            format!("GET {path} is not the playground's page: {}", reply.status)
        });
    }

    // The browser variant's files, pulled offline from tmp/hf by the first request: their lengths are what a tab
    // downloads (`x_browser`).
    let profile = probes.send("GET", "/v1/browser/decider-0.8b/profile", None)?;
    let profile_name =
        serde_json::from_str::<Value>(&profile.body).unwrap_or_default()["name"].clone();
    probes.expect(
        profile.status == 200 && profile_name == "decider-0.8b-v1",
        || {
            format!(
                "GET /v1/browser/decider-0.8b/profile: {} {}",
                profile.status, profile.body
            )
        },
    );
    let mut bytes = 0;
    for file in ["model.onnx", "model.onnx.data", "tokenizer.json"] {
        let path = format!("/v1/browser/decider-0.8b/{file}");
        let reply = probes.send("HEAD", &path, None)?;
        let length = reply
            .header("content-length")
            .and_then(|length| length.parse::<i64>().ok());
        probes.expect(reply.status == 200 && length.is_some(), || {
            format!("HEAD {path}: {} {:?}", reply.status, reply.headers)
        });
        bytes += length.unwrap_or_default();
    }
    let size = got
        .iter()
        .find(|(name, ..)| name == "decider-0.8b")
        .and_then(|(_, size, _)| *size);
    probes.expect(size == Some(bytes), || {
        format!("decider-0.8b's browser files hold {bytes} bytes, x_browser says {size:?}")
    });
    println!(
        "e2e public: {} HTTP probes of {url}: the library listed, POST /v1/systemone 403, /health names no model, \
         traversal and unknown paths 404, other methods 405, no CORS header, no path in a body; {problems} problems",
        probes.sent,
        problems = probes.problems.len()
    );
    Ok(probes.problems)
}

/// One HTTP exchange: the status, the headers (names lowercased) and the body.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value.as_str())
    }
}

/// The public suite's probes of one server, each sent with curl as written (`--path-as-is`: `..` and percent escapes
/// reach the server untouched) and from another origin, so a CORS header would show. Every reply is checked for the
/// Q13 headers, for any `Access-Control-*` header and for the user's home in its body.
struct Probes<'a> {
    sandbox: &'a Sandbox,
    url: &'a str,
    problems: Vec<String>,
    sent: usize,
}

impl<'a> Probes<'a> {
    fn new(sandbox: &'a Sandbox, url: &'a str) -> Probes<'a> {
        Probes {
            sandbox,
            url,
            problems: Vec::new(),
            sent: 0,
        }
    }

    /// `method path`, with a JSON `body` when given.
    fn send(&mut self, method: &str, path: &str, body: Option<&str>) -> Result<Reply> {
        let mut curl = self.sandbox.command("curl");
        curl.args(["-sS", "--path-as-is", "--max-time", "120"])
            .args(["-H", "origin: https://elsewhere.example"]);
        if method == "HEAD" {
            curl.arg("-I");
        } else {
            curl.args(["-i", "-X", method]);
        }
        if let Some(body) = body {
            curl.args([
                "-H",
                "content-type: application/json",
                "--data-binary",
                body,
            ]);
        }
        let output = curl
            .arg(format!("{}{path}", self.url))
            .output()
            .context("running curl")?;
        if !output.status.success() {
            bail!(
                "curl {method} {path} failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let mut lines = head.lines();
        let status = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .with_context(|| format!("{method} {path}: no status in {head:?}"))?;
        let reply = Reply {
            status,
            headers: lines
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
                .collect(),
            body: body.to_string(),
        };
        self.sent += 1;
        let what = format!("{method} {path}");
        for (name, value) in [
            ("cross-origin-opener-policy", "same-origin"),
            ("cross-origin-embedder-policy", "require-corp"),
            ("cross-origin-resource-policy", "same-origin"),
        ] {
            let got = reply.header(name);
            self.expect(got == Some(value), || format!("{what}: {name} is {got:?}"));
        }
        let cors: Vec<&str> = reply
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .filter(|name| name.starts_with("access-control-"))
            .collect();
        self.expect(cors.is_empty(), || format!("{what} answers {cors:?}"));
        let home = self.sandbox.real_home().display().to_string();
        self.expect(!reply.body.contains(&home), || {
            format!("{what}: the body names {home}")
        });
        Ok(reply)
    }

    fn expect(&mut self, ok: bool, problem: impl FnOnce() -> String) {
        if !ok {
            self.problems.push(problem());
        }
    }

    /// R3.2: an error body names no filesystem path and no upstream URL.
    fn tells_no_path(&mut self, what: &str, reply: &Reply) {
        for told in ["/", "\\", "hf.co", "huggingface"] {
            self.expect(!reply.body.contains(told), || {
                format!("{what}: {} names {told:?}", reply.body)
            });
        }
    }
}

pub fn design(sandbox: &Sandbox) -> Result<()> {
    let mut failures = context(sandbox);
    for problem in &failures {
        println!("e2e design: context: {problem}");
    }
    if failures.is_empty() {
        println!(
            "e2e design: context: PRODUCT.md, DESIGN.md, surface brief, config and hook in place"
        );
    }
    let scans = detect(sandbox)?;
    let finish = finish(sandbox, &scans);
    for problem in &finish {
        println!("e2e design: finish: {problem}");
    }
    if finish.is_empty() {
        println!(
            "e2e design: finish: critique record, audit with no P0 or P1, clean scans, hook enabled"
        );
    }
    failures.extend(scans);
    failures.extend(finish);
    if !failures.is_empty() {
        bail!("e2e design failed:\n  {}", failures.join("\n  "));
    }
    Ok(())
}

/// The audit line that says no blocking or major issue is left.
const AUDIT_CLEAN: &str = "P0: 0 · P1: 0";

/// R7.7: every problem with the finish of the playground: a fresh-context critique left its record under
/// `.impeccable/critique/`, `docs/design/audit.md` holds the audit with no P0 or P1 left, this run's detect scans
/// (`scan_failures`, from [`detect`]) found no primary finding, and the design hook is still enabled.
fn finish(sandbox: &Sandbox, scan_failures: &[String]) -> Vec<String> {
    let root = sandbox.repo_root();
    let mut problems = Vec::new();
    let critique = root.join(".impeccable/critique");
    let records = std::fs::read_dir(&critique)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_file() && entry.file_name() != "ignore.md")
                .count()
        })
        .unwrap_or(0);
    if records == 0 {
        problems.push(format!("{} holds no critique record", critique.display()));
    }
    let audit = root.join("docs/design/audit.md");
    match std::fs::read_to_string(&audit) {
        Ok(text) if text.lines().any(|line| line.trim() == AUDIT_CLEAN) => {}
        Ok(_) => problems.push(format!("{} has no `{AUDIT_CLEAN}` line", audit.display())),
        Err(_) => problems.push(format!("{} is missing", audit.display())),
    }
    if !scan_failures.is_empty() {
        problems.push(format!(
            "{} impeccable detect scans of R6.2's URLs report primary findings or failed",
            scan_failures.len()
        ));
    }
    match read_json(&root.join(".impeccable/config.json")) {
        Ok(config) if config["hook"]["enabled"] == json!(true) => {}
        Ok(_) => problems.push(".impeccable/config.json does not enable the hook".into()),
        Err(err) => problems.push(format!("{err:#}")),
    }
    problems
}

/// R6.1: every problem with the /impeccable context of the playground.
fn context(sandbox: &Sandbox) -> Vec<String> {
    let root = sandbox.repo_root();
    let mut problems = Vec::new();
    for file in ["PRODUCT.md", "DESIGN.md"] {
        if !root.join(file).is_file() {
            problems.push(format!("{file} is missing at the repo root"));
        }
    }
    let impeccable = || {
        let mut cmd = sandbox.command(sandbox.tmp().join("impeccable/bin/0.1.5/impeccable"));
        cmd.current_dir(root);
        cmd
    };
    // Doctor's "mention" findings are its advisory tier (a sidecar whose mtime trails DESIGN.md's after a checkout,
    // for one); they are printed, and only the graver findings fail the context.
    match impeccable().args(["doctor", "--json"]).output() {
        Ok(out) => match serde_json::from_slice::<Value>(&out.stdout) {
            Ok(report) if out.status.success() => {
                let (mentions, failures): (Vec<&Value>, Vec<&Value>) = report["findings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .partition(|finding| finding["severity"] == json!("mention"));
                for mention in mentions {
                    println!("e2e design: doctor mentions {}", mention["summary"]);
                }
                if !failures.is_empty() {
                    problems.push(format!("impeccable doctor reports {}", json!(failures)));
                }
            }
            Ok(report) => {
                problems.push(format!("impeccable doctor reports {}", report["findings"]))
            }
            Err(err) => problems.push(format!(
                "impeccable doctor --json is not JSON ({}): {err}",
                out.status
            )),
        },
        Err(err) => problems.push(format!(
            "running impeccable doctor: {err}; run `cargo xtask fetch`"
        )),
    }
    match impeccable()
        .args(["surface-brief", "read", SURFACE])
        .output()
    {
        Ok(out) => {
            let brief = String::from_utf8_lossy(&out.stdout);
            if !out.status.success() || !brief.contains("Mode: Operate") {
                problems.push(format!(
                    "the {SURFACE} surface brief is not in Operate mode"
                ));
            }
            for block in CONTRACT_BLOCKS {
                if !brief
                    .lines()
                    .any(|line| line.starts_with(&format!("{block}:")))
                {
                    problems.push(format!(
                        "the {SURFACE} direction contract has no {block} block"
                    ));
                }
            }
        }
        Err(err) => problems.push(format!("running impeccable surface-brief read: {err}")),
    }
    match read_json(&root.join(".impeccable/config.json")) {
        Ok(config) => {
            if config["hook"]["enabled"] != json!(true) {
                problems.push(".impeccable/config.json does not enable the hook".into());
            }
            if config["buildPath"] != json!("code") {
                problems.push(".impeccable/config.json buildPath is not \"code\"".into());
            }
        }
        Err(err) => problems.push(format!("{err:#}")),
    }
    match read_json(&root.join(".claude/settings.local.json")) {
        Ok(settings) => {
            let hooked = settings["hooks"]["PostToolUse"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
                .filter_map(|hook| hook["command"].as_str())
                .any(|command| command.contains("impeccable") && command.ends_with(" hook"));
            if !hooked {
                problems.push(
                    ".claude/settings.local.json holds no impeccable PostToolUse hook".into(),
                );
            }
        }
        Err(err) => problems.push(format!("{err:#}")),
    }
    problems
}

/// R6.2, R2.7, R3.6: `impeccable detect` exits 0 on the empty, loaded, results and 422 states, the run-command state
/// (a model the server has not pulled) and the public playground at both viewports, and on the in-tab download and
/// results states the `@design` test freezes from a real run in the tab. Each JSON report lands in
/// `tmp/evals/design/`; returns one line per failed scan.
fn detect(sandbox: &Sandbox) -> Result<Vec<String>> {
    let ardana = playground::build(sandbox)?;
    let out = sandbox.tmp().join("evals/design");
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }
    std::fs::create_dir_all(&out)?;
    let ticket = read_json(&sandbox.repo_root().join(TICKET))?;
    let mut invalid = ticket.clone();
    invalid["questions"]["department"]["criteria"] = json!(["billing"]);
    // A library model the design server has not pulled and no tab can run: the page shows the ardana CLI's command.
    let mut lacked = ticket.clone();
    lacked["model"] = json!("decider-4b");
    let server = Server::start(sandbox, &ardana, "design", None)?;
    let public = Server::start_public(sandbox, &ardana, "design-public", false)?;
    warm(sandbox, &server.url, &ticket)?;
    let states = [
        ("empty", format!("{}/", server.url)),
        (
            "loaded",
            format!("{}/#share/{}", server.url, share(&ticket)),
        ),
        (
            "results",
            format!("{}/?autorun=1#share/{}", server.url, share(&ticket)),
        ),
        (
            "error-422",
            format!("{}/?autorun=1#share/{}", server.url, share(&invalid)),
        ),
        (
            "run-command",
            format!("{}/#share/{}", server.url, share(&lacked)),
        ),
        (
            "public",
            format!("{}/#share/{}", public.url, share(&ticket)),
        ),
    ];
    let mut failures = Vec::new();
    for (state, url) in &states {
        for (project, width, height) in VIEWPORTS {
            scan(
                sandbox,
                &out,
                state,
                project,
                &format!("{width}x{height}"),
                url,
                &mut failures,
            )?;
        }
    }
    public.stop()?;
    // The in-tab states: the `@design` test runs decider-0.8b's browser variant on this server at each viewport and
    // writes the frozen pages into `out`.
    let status = playwright(sandbox)
        .args([
            "test",
            "--grep",
            DESIGN_TAG,
            "--reporter",
            "list",
            "--output",
        ])
        .arg(sandbox.tmp().join("playwright/design-results"))
        .env("ARDANA_BASE_URL", &server.url)
        .env("ARDANA_REPO_ROOT", sandbox.repo_root())
        .env("ARDANA_DESIGN_OUT", &out)
        .status()
        .context("running playwright test; run `cargo xtask fetch`")?;
    server.stop()?;
    if !status.success() {
        failures.push(format!(
            "the {DESIGN_TAG} Playwright test failed ({status})"
        ));
        return Ok(failures);
    }
    for state in FROZEN_STATES {
        for (project, width, height) in VIEWPORTS {
            let page = out.join(format!("{state}-{project}.html"));
            let url = format!("file://{}", page.display());
            scan(
                sandbox,
                &out,
                state,
                project,
                &format!("{width}x{height}"),
                &url,
                &mut failures,
            )?;
        }
    }
    Ok(failures)
}

/// `impeccable detect` of `url` at `viewport`, its JSON report written to `<out>/<state>-<project>.json`; a scan that
/// does not exit 0 adds a line to `failures`.
fn scan(
    sandbox: &Sandbox,
    out: &Path,
    state: &str,
    project: &str,
    viewport: &str,
    url: &str,
    failures: &mut Vec<String>,
) -> Result<()> {
    let output = sandbox
        .command(sandbox.tmp().join("impeccable/bin/0.1.5/impeccable"))
        .current_dir(sandbox.repo_root())
        .args(["detect", "--json", "--viewport", viewport])
        .arg(url)
        .output()
        .context("running impeccable detect")?;
    let report = out.join(format!("{state}-{project}.json"));
    std::fs::write(&report, &output.stdout)?;
    let findings = serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .and_then(|v| v.as_array().map(Vec::len));
    let code = output.status.code().unwrap_or(-1);
    println!(
        "e2e design: detect {state} {viewport}: exit {code}, {} findings ({})",
        findings.map_or("?".to_string(), |n| n.to_string()),
        report.display()
    );
    if code != 0 {
        failures.push(format!(
            "impeccable detect {state} at {viewport} exited {code}: {}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Loads decider-2b with one request, so the results states do not wait on the first load.
fn warm(sandbox: &Sandbox, url: &str, request: &Value) -> Result<()> {
    let output = sandbox
        .command("curl")
        .args([
            "-fsS",
            "--max-time",
            "120",
            "-H",
            "content-type: application/json",
            "--data-binary",
        ])
        .arg(request.to_string())
        .arg(format!("{url}/v1/systemone"))
        .output()
        .context("running curl")?;
    if !output.status.success() {
        bail!(
            "warming the model failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// A Jev share link payload for a request, as Jev's playground writes it.
pub fn share(request: &Value) -> String {
    let document = match &request["state"] {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    let payload = json!({
        "apiVersion": "v1",
        "documentText": document,
        "promptsText": serde_json::to_string_pretty(&request["questions"]).unwrap_or_default(),
        "selectedModels": [request["model"].as_str().unwrap_or("jev-latest")],
    });
    lz_str::compress_to_encoded_uri_component(payload.to_string().as_str())
}
