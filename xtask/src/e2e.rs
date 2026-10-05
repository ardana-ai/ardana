//! `cargo xtask e2e <suite>`: end-to-end suites, each run under the home guard. The API suites (`jevcompat`, `sdk`,
//! `jevbench`) run their harness from `tmp/` against a release `ardana serve` on decider:2b; the browser suites
//! (`playground`, `standalone`, `design`) live in [`crate::browser`].

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::browser;
use crate::sandbox::{Sandbox, cargo};
use crate::serve::{Server, build_ardana};

pub const SUITES: &[&str] = &[
    "smoke",
    "rust",
    "jevcompat",
    "sdk",
    "jevbench",
    "playground",
    "standalone",
    "design",
];

/// The request fixture the SDK suite sends.
const SDK_REQUEST: &str = "tests/fixtures/requests/ticket.json";
/// JevBench's public items, in the order its results report them.
const JEVBENCH_TASKS: &[&str] = &[
    "datasets/public/easy.jsonl",
    "datasets/public/original.jsonl",
    "datasets/public/hard.jsonl",
];

pub fn run(sandbox: &Sandbox, suite: &str) -> Result<()> {
    match suite {
        "smoke" => smoke(sandbox),
        "rust" => rust(sandbox),
        "jevcompat" => jevcompat(sandbox),
        "sdk" => sdk(sandbox),
        "jevbench" => jevbench(sandbox),
        "playground" => browser::playground(sandbox),
        "standalone" => browser::standalone(sandbox),
        "design" => browser::design(sandbox),
        other => bail!("unknown e2e suite `{other}`; suites: {}", SUITES.join(", ")),
    }
}

/// Builds and runs `ardana --version`.
fn smoke(sandbox: &Sandbox) -> Result<()> {
    let output = sandbox
        .command(cargo())
        .current_dir(sandbox.repo_root())
        .args(["run", "--quiet", "--package", "ardana", "--", "--version"])
        .output()
        .context("running ardana --version")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !stdout.starts_with("ardana ") {
        bail!(
            "ardana --version failed ({}): {}{}",
            output.status,
            stdout.trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("e2e smoke: {}", stdout.trim());
    Ok(())
}

/// Every workspace test, including the `#[ignore]`d end-to-end tests on real models.
fn rust(sandbox: &Sandbox) -> Result<()> {
    let status = sandbox
        .command(cargo())
        .current_dir(sandbox.repo_root())
        .args(["test", "--workspace", "--", "--include-ignored"])
        .status()
        .context("running cargo test")?;
    if !status.success() {
        bail!("cargo test --workspace -- --include-ignored failed ({status})");
    }
    Ok(())
}

/// An emptied `tmp/evals/<name>`.
fn evals_dir(sandbox: &Sandbox, name: &str) -> Result<std::path::PathBuf> {
    let dir = sandbox.tmp().join("evals").join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("emptying {}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// R5.1: `jevcompat test` against an open server and against one that requires a key (read from `ARDANA_API_KEY`),
/// so both conditional auth MUSTs are tested; each run must be conformant. Reports go to `tmp/evals/jevcompat/`.
fn jevcompat(sandbox: &Sandbox) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let out = evals_dir(sandbox, "jevcompat")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let key = format!("ardana-e2e-{nanos:x}");
    for (label, key) in [("open", None), ("key", Some(key.as_str()))] {
        let server = Server::start(sandbox, &ardana, &format!("jevcompat-{label}"), key)?;
        let report = out.join(format!("{label}.json"));
        let mut test = sandbox.command("jevcompat");
        test.args(["test", &server.url, "--json"]).arg(&report);
        if let Some(key) = key {
            test.args(["--key-env", "JEVCOMPAT_KEY"])
                .env("JEVCOMPAT_KEY", key);
        }
        let status = test.status().context("running jevcompat test")?;
        server.stop()?;
        let summary = read_json(&report)?["summary"].clone();
        let count = |level: &str, what: &str| summary[level][what].as_u64().unwrap_or_default();
        println!(
            "e2e jevcompat ({label}): {} · MUST {}/{} passed, {} failed, {} skipped · SHOULD {}/{} passed",
            summary["verdict"].as_str().unwrap_or("?"),
            count("MUST", "passed"),
            count("MUST", "tested"),
            count("MUST", "failed"),
            count("MUST", "skipped"),
            count("SHOULD", "passed"),
            count("SHOULD", "tested"),
        );
        if !status.success()
            || summary["conformant"] != Value::Bool(true)
            || count("MUST", "failed") != 0
        {
            bail!(
                "jevcompat ({label}) is not conformant ({status}); see {}",
                report.display()
            );
        }
    }
    Ok(())
}

/// R5.2: `typesafe-sdk` from `tmp/py/sdk`, pointed at the server by `TYPESAFE_BASE_URL`, parses the ticket answers.
fn sdk(sandbox: &Sandbox) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let server = Server::start(sandbox, &ardana, "sdk", None)?;
    let root = sandbox.repo_root();
    let status = sandbox
        .command(sandbox.tmp().join("py/sdk/bin/python"))
        .current_dir(root)
        .arg(root.join("xtask/scripts/sdk_ticket.py"))
        .arg(root.join(SDK_REQUEST))
        .env("TYPESAFE_BASE_URL", &server.url)
        .env("TYPESAFE_API_KEY", "ardana-e2e-unused")
        .status()
        .context("running the SDK check")?;
    server.stop()?;
    if !status.success() {
        bail!("typesafe-sdk did not parse the answers to {SDK_REQUEST} ({status})");
    }
    Ok(())
}

/// R5.3: JevBench's 231 public items through its `typesafe` adapter, run from the `tmp/src/jevbench` clone with the
/// `tmp/py/jevbench` venv; results, ledger, raw responses and summary land in `tmp/evals/jevbench/`. Every request
/// must succeed; accuracy is reported, not gated.
fn jevbench(sandbox: &Sandbox) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let out = evals_dir(sandbox, "jevbench")?;
    let clone = sandbox.tmp().join("src/jevbench");
    let python = sandbox.tmp().join("py/jevbench/bin/python");
    let tasks = JEVBENCH_TASKS
        .iter()
        .map(|file| clone.join(file).display().to_string())
        .collect::<Vec<_>>()
        .join(",");
    let results = out.join("results.jsonl");
    let ledger = out.join("ledger.jsonl");
    let server = Server::start(sandbox, &ardana, "jevbench", None)?;
    let status = sandbox
        .command(&python)
        .current_dir(&clone)
        .args([
            "-m",
            "jevbench.cli",
            "run",
            "--adapter",
            "typesafe",
            "--key-env",
            "",
            "--endpoint",
        ])
        .arg(&server.url)
        .arg("--tasks")
        .arg(&tasks)
        .arg("--results")
        .arg(&results)
        .arg("--ledger")
        .arg(&ledger)
        .arg("--raw-dir")
        .arg(out.join("raw"))
        .status()
        .context("running jevbench run")?;
    server.stop()?;

    let summary = sandbox
        .command(&python)
        .current_dir(&clone)
        .args(["-m", "jevbench.cli", "summarize", "--tasks"])
        .arg(&tasks)
        .arg("--results")
        .arg(&results)
        .arg("--ledger")
        .arg(&ledger)
        .output()
        .context("running jevbench summarize")?;
    if !summary.status.success() {
        bail!(
            "jevbench summarize failed ({}): {}",
            summary.status,
            String::from_utf8_lossy(&summary.stderr)
        );
    }
    std::fs::write(out.join("summary.json"), &summary.stdout)?;
    let summary: Value =
        serde_json::from_slice(&summary.stdout).context("parsing the JevBench summary")?;

    let records = std::fs::read_to_string(&results)
        .with_context(|| format!("reading {}", results.display()))?;
    let records: Vec<Value> = records
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .context("parsing the JevBench results")?;
    let failed: Vec<&Value> = records.iter().filter(|r| r["status"] != "ok").collect();
    let planned = summary["n_planned"].as_u64().unwrap_or_default();
    println!(
        "e2e jevbench: {} of {planned} requests, {} failed; accuracy {} ({} of {} scorable), macro accuracy {}; {}",
        records.len(),
        failed.len(),
        summary["accuracy"],
        summary["n_correct"],
        summary["n_scorable"],
        summary["macro_accuracy"],
        out.display()
    );
    for record in failed.iter().take(5) {
        println!("  failed {}: {}", record["task_id"], record["error"]);
    }
    if !status.success() || records.len() as u64 != planned || !failed.is_empty() {
        bail!("jevbench run had failed or missing requests ({status})");
    }
    Ok(())
}

pub(crate) fn read_json(path: &Path) -> Result<Value> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}
