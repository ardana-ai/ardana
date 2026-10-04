//! The browser suites (Q24). `playground`: the release `ardana` from `cargo xtask build`, copied alone into
//! `tmp/e2e/playground-binary` and serving decider-2b while the playground's `dist/` is moved away (plus one server
//! on an empty registry, for `first_run`, `browser_stop` and `browser_recover`, one more for `browser_run`,
//! `insecure_origin` and `browser_resume`, one over an empty Hub cache for `browser_pull`, and one more empty one whose
//! registry `pull_while_open` pulls into with `ardana pull`), driven by the Playwright cases in
//! `e2e/playground` on the installed Chrome; plus the placeholder build of `ardana-server`. `standalone`: the standalone
//! playground (`cargo xtask build-playground`) as static files under `/playground/`, the library document served
//! beside it at `/models.json` from a copy of the snapshot the cases rewrite, its browser files from a stand-in of
//! Hugging Face serving `tmp/hf` offline on another origin, driven by the `@standalone` Playwright cases. `design`:
//! the /impeccable context of the playground, `impeccable detect` on its five URL states, its two in-tab states
//! (frozen by the `@design` Playwright test) and the standalone build's library-unavailable state at 1280x800 and
//! 390x844, and the finish (critique record, audit, clean scans, hook on).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::e2e::read_json;
use crate::playground;
use crate::sandbox::{Sandbox, cargo};
use crate::serve::{Server, build_ardana};

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
/// The Playwright cases of the standalone suite, screenshots alike.
pub const STANDALONE_CASES: &[&str] = &[
    "standalone_first_run",
    "standalone_browser_run",
    "standalone_run_command",
    "standalone_share",
    "standalone_library_update",
    "standalone_library_unavailable",
];
/// Playwright project name and viewport, as `playwright.config.ts` and `impeccable detect` use them.
pub const VIEWPORTS: &[(&str, u32, u32)] = &[("desktop", 1280, 800), ("mobile", 390, 844)];

/// The request the loaded and results states come from.
const TICKET: &str = "tests/fixtures/requests/ticket.json";
const PLAYGROUND_DIR: &str = "e2e/playground";
/// The Playwright tag of the design suite's tests, which the playground suite leaves out.
const DESIGN_TAG: &str = "@design";
/// The Playwright tag of the standalone suite's tests, which the playground suite leaves out.
const STANDALONE_TAG: &str = "@standalone";
/// Where the standalone suite's page is served.
const STANDALONE_URL: &str = "/playground/";
/// The in-tab states the `@design` test freezes into `<state>-<project>.html` for `impeccable detect`.
const FROZEN_STATES: &[&str] = &["browser-download", "browser-results"];
/// The standalone build's state while its library document does not arrive, scanned at the page's URL.
const LIBRARY_UNAVAILABLE: &str = "library-unavailable";
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
                &format!("{DESIGN_TAG}|{STANDALONE_TAG}"),
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

/// R2.6, R6.1: the standalone playground built with its hub at a stand-in of Hugging Face and its library at the
/// default `/models.json` (R6.1: the build bakes no library: it writes no `tmp/playground/library.json`, and its dist
/// names no repository of the snapshot), served as static files under `/playground/` with the snapshot's copy at
/// `/models.json`, and the `@standalone` cases on it at both viewports; the release `ardana` gives `ardana run`'s own
/// answers to compare the tab's with.
pub fn standalone(sandbox: &Sandbox) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let hosts = Hosts::start(sandbox)?;
    let dist =
        playground::build_standalone(sandbox, STANDALONE_URL, &hosts.hub, playground::LIBRARY)?;
    let baked = sandbox.tmp().join("playground/library.json");
    if baked.exists() {
        bail!("the standalone build wrote {}", baked.display());
    }
    let snapshot = std::fs::read_to_string(sandbox.snapshot())?;
    let repositories: Vec<&str> = snapshot
        .split('"')
        .filter(|word| word.starts_with("hf.co/"))
        .collect();
    for file in files_under(&dist)? {
        let bytes = std::fs::read(&file)?;
        for repository in &repositories {
            if bytes
                .windows(repository.len())
                .any(|w| w == repository.as_bytes())
            {
                bail!(
                    "the standalone build baked {repository} into {}",
                    file.display()
                );
            }
        }
    }
    println!(
        "e2e standalone: no library baked ({} files of {} name none of the snapshot's repositories)",
        files_under(&dist)?.len(),
        dist.display()
    );
    clear_screens(sandbox, STANDALONE_CASES)?;
    let status = playwright(sandbox)
        .args([
            "test",
            "--grep",
            STANDALONE_TAG,
            "--reporter",
            "list",
            "--output",
        ])
        .arg(sandbox.tmp().join("playwright/standalone-results"))
        .env("ARDANA_BASE_URL", &hosts.site)
        .env("ARDANA_HUB_URL", &hosts.hub)
        .env("ARDANA_SITE_LIBRARY", &hosts.library)
        .env("ARDANA_BIN", &ardana)
        .env("ARDANA_REPO_ROOT", sandbox.repo_root())
        .status()
        .context("running playwright test; run `cargo xtask fetch`")?;
    drop(hosts);
    if !status.success() {
        bail!("the {STANDALONE_TAG} Playwright cases failed ({status})");
    }
    let missing = screenshots(sandbox, STANDALONE_CASES);
    if !missing.is_empty() {
        bail!("screenshots: {}", missing.join("; "));
    }
    println!(
        "e2e standalone: screenshots of {} cases at 1280 and 390 wide in {}",
        STANDALONE_CASES.len(),
        sandbox.tmp().join("screens").display()
    );
    Ok(())
}

/// The standalone suite's two hosts (`e2e/playground/standalone-hosts.mjs`), each of its own origin: the site serving
/// `tmp/playground/dist` under `/playground/` and, at `/models.json`, the file `library` (a copy of the snapshot the
/// cases rewrite, add to and take away), and the stand-in of Hugging Face serving `tmp/hf/hub`. Dropping it stops both.
struct Hosts {
    child: Child,
    site: String,
    hub: String,
    library: PathBuf,
}

impl Hosts {
    fn start(sandbox: &Sandbox) -> Result<Hosts> {
        let dir = sandbox.repo_root().join(PLAYGROUND_DIR);
        let library = sandbox.tmp().join("playground/models.json");
        std::fs::create_dir_all(library.parent().unwrap())?;
        std::fs::copy(sandbox.snapshot(), &library)
            .with_context(|| format!("copying the snapshot to {}", library.display()))?;
        let mut child = sandbox
            .command("node")
            .current_dir(&dir)
            .arg(dir.join("standalone-hosts.mjs"))
            .arg(sandbox.tmp().join("playground/dist"))
            .arg(sandbox.tmp().join("hf/hub"))
            .arg(&library)
            .stdout(Stdio::piped())
            .spawn()
            .context("starting the standalone hosts with node")?;
        let mut line = String::new();
        let read = child
            .stdout
            .take()
            .map(|stdout| BufReader::new(stdout).read_line(&mut line));
        let urls: Value = match read {
            Some(Ok(_)) => serde_json::from_str(&line).unwrap_or_default(),
            _ => Value::Null,
        };
        let (Some(site), Some(hub)) = (urls["site"].as_str(), urls["hub"].as_str()) else {
            let _ = child.kill();
            bail!("the standalone hosts did not start: {line:?}");
        };
        println!(
            "e2e standalone: the page at {site}{STANDALONE_URL}, its library at {site}/models.json ({}), the hub at {hub}",
            library.display()
        );
        Ok(Hosts {
            site: site.to_string(),
            hub: hub.to_string(),
            library,
            child,
        })
    }
}

impl Drop for Hosts {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every file under `dir`, at any depth.
fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(files_under(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
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

/// R6.2, R2.7, R3.6, R6.8: `impeccable detect` exits 0 on the empty, loaded, results and 422 states, the run-command
/// state (a model the server has not pulled) at both viewports, on the in-tab download and results states the
/// `@design` test freezes from a real run in the tab, and on the standalone build's library-unavailable state (its
/// `/models.json` a 404, served by the standalone suite's hosts). Each JSON report lands in `tmp/evals/design/`;
/// returns one line per failed scan.
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
    // The standalone build with no library document: the page lists nothing, says the library is unavailable under
    // the picker and holds Run; the network goes idle once the 404 is in, so the page's URL is the state.
    let hosts = Hosts::start(sandbox)?;
    playground::build_standalone(sandbox, STANDALONE_URL, &hosts.hub, playground::LIBRARY)?;
    std::fs::remove_file(&hosts.library)
        .with_context(|| format!("removing {}", hosts.library.display()))?;
    let url = format!("{}{STANDALONE_URL}", hosts.site);
    for (project, width, height) in VIEWPORTS {
        scan(
            sandbox,
            &out,
            LIBRARY_UNAVAILABLE,
            project,
            &format!("{width}x{height}"),
            &url,
            &mut failures,
        )?;
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
