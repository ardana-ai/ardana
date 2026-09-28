//! `cargo xtask e2e <suite>`: end-to-end suites, each run under the home guard.

use anyhow::{Context, Result, bail};

use crate::sandbox::{Sandbox, cargo};

pub const SUITES: &[&str] = &["smoke"];

pub fn run(sandbox: &Sandbox, suite: &str) -> Result<()> {
    match suite {
        "smoke" => smoke(sandbox),
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
