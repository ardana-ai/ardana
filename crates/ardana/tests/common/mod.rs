//! Shared helpers of the `ardana` end-to-end tests: the Hub files `cargo xtask fetch` put into `tmp/hf`, the request
//! fixtures and `ardana run`.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// A file of the pinned snapshot of `repo` (`org/name`) in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
pub fn hf_file(repo: &str, name: &str) -> Result<PathBuf> {
    let hf_home = std::env::var_os("HF_HOME").context("HF_HOME is not set")?;
    let dir = PathBuf::from(hf_home)
        .join("hub")
        .join(format!("models--{}", repo.replace('/', "--")));
    let rev = std::fs::read_to_string(dir.join("refs/main")).with_context(|| {
        format!(
            "{} has no refs/main; run `cargo xtask fetch`",
            dir.display()
        )
    })?;
    let path = dir.join("snapshots").join(rev.trim()).join(name);
    ensure!(
        path.is_file(),
        "{} is missing; run `cargo xtask fetch`",
        path.display()
    );
    Ok(path)
}

/// A request fixture in `tests/fixtures/requests`.
pub fn request(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/requests")
        .join(name)
}

/// Runs `ardana run <args>`, prints its output and parses the response.
pub fn ardana_run<I, S>(args: I) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ardana"));
    cmd.arg("run").args(args);
    let output = cmd.output().context("running ardana run")?;
    ensure!(
        output.status.success(),
        "{cmd:?} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    println!("{cmd:?}:\n{stdout}");
    serde_json::from_str(&stdout).context("ardana run printed JSON")
}

/// The sum of a choice answer's probabilities.
pub fn probability_sum(answer: &Value) -> Result<f64> {
    let probabilities = answer["probabilities"]
        .as_object()
        .context("probabilities")?;
    Ok(probabilities.values().filter_map(Value::as_f64).sum())
}
