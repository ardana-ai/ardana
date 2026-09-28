//! The browser suites (Q24). `playground`: the release `ardana` from `cargo xtask build`, copied alone into
//! `tmp/e2e/playground-binary` and serving decider-2b while the playground's `dist/` is moved away, driven by the
//! Playwright cases in `e2e/playground` on the installed Chrome; plus the placeholder build of `ardana-server`.
//! `design`: the /impeccable context of the playground, `impeccable detect` on its four states at 1280x800 and
//! 390x844, and the finish (critique record, audit, clean scans, hook on).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::e2e::read_json;
use crate::playground;
use crate::sandbox::{Sandbox, cargo};
use crate::serve::Server;

/// The Playwright cases; each saves at least one screenshot per viewport under `tmp/screens/<case>/`.
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
];
/// Playwright project name and viewport, as `playwright.config.ts` and `impeccable detect` use them.
pub const VIEWPORTS: &[(&str, u32, u32)] = &[("desktop", 1280, 800), ("mobile", 390, 844)];

/// The request the loaded and results states come from.
const TICKET: &str = "tests/fixtures/requests/ticket.json";
const PLAYGROUND_DIR: &str = "e2e/playground";
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

    let screens = sandbox.tmp().join("screens");
    for case in CASES {
        let case_dir = screens.join(case);
        if case_dir.exists() {
            std::fs::remove_dir_all(&case_dir)?;
        }
    }
    let status = {
        let _moved = MovedAway::new(
            &sandbox.repo_root().join("crates/ardana-playground/dist"),
            &sandbox.tmp().join("e2e/playground-dist-moved"),
        )?;
        let server = Server::start(sandbox, &copy, "playground", None)?;
        let root = sandbox.repo_root();
        let status = sandbox
            .command(
                root.join(PLAYGROUND_DIR)
                    .join("node_modules/.bin/playwright"),
            )
            .current_dir(root.join(PLAYGROUND_DIR))
            .arg("test")
            .env("ARDANA_BASE_URL", &server.url)
            .env("ARDANA_REPO_ROOT", root)
            .status()
            .context("running playwright test; run `cargo xtask fetch`")?;
        server.stop()?;
        status
    };
    if !status.success() {
        bail!("playwright test failed ({status}); report in tmp/playwright/report");
    }
    screenshots(sandbox)
}

/// R6.7: every case left 1280- and 390-wide screenshots (`<project>.png` or `<state>-<project>.png`), and Playwright
/// wrote its results under `tmp/playwright/`.
fn screenshots(sandbox: &Sandbox) -> Result<()> {
    let mut missing = Vec::new();
    for case in CASES {
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
    match impeccable().args(["doctor", "--json"]).output() {
        Ok(out) => match serde_json::from_slice::<Value>(&out.stdout) {
            Ok(report) if out.status.success() && report["findings"] == json!([]) => {}
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

/// R6.2: `impeccable detect` exits 0 on the empty, loaded, results and 422 states at both viewports. Each JSON
/// report lands in `tmp/evals/design/`; returns one line per failed scan.
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
    ];
    let mut failures = Vec::new();
    for (state, url) in &states {
        for (project, width, height) in VIEWPORTS {
            let viewport = format!("{width}x{height}");
            let output = sandbox
                .command(sandbox.tmp().join("impeccable/bin/0.1.5/impeccable"))
                .current_dir(sandbox.repo_root())
                .args(["detect", "--json", "--viewport", &viewport])
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
        }
    }
    server.stop()?;
    Ok(failures)
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
