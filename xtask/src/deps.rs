//! `cargo xtask check-deps`: the Q22 members and their dependency direction.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::sandbox::cargo;

/// The Q22 workspace members, the playground's engine module and the Candle runtime.
pub const MEMBERS: &[&str] = &[
    "ardana",
    "ardana-api",
    "ardana-candle",
    "ardana-core",
    "ardana-llama",
    "ardana-registry",
    "ardana-server",
    "ardana-playground",
    "ardana-engine",
    "xtask",
];

/// Whether workspace crate `from` may depend on workspace crate `to` (R1.1).
fn allowed(from: &str, to: &str) -> bool {
    match from {
        "ardana-api" => false,
        "ardana-core" => to == "ardana-api",
        "ardana-llama" | "ardana-candle" | "ardana-registry" => to == "ardana-core",
        // The server reaches runtimes through `ardana-core`'s traits alone; the binary wires the concrete ones in.
        "ardana-server" => !matches!(to, "ardana-llama" | "ardana-candle"),
        // The page's own wasm carries the API's types alone: the core's tokenizer, planner and readout reach the tab in
        // the engine module, which the page imports on an "In browser" pick.
        "ardana-playground" => to == "ardana-api",
        "ardana-engine" => matches!(to, "ardana-api" | "ardana-core"),
        _ => true,
    }
}

/// `cargo metadata --no-deps` for the workspace at `repo_root`.
pub fn metadata(repo_root: &Path) -> Result<Value> {
    let output = std::process::Command::new(cargo())
        .current_dir(repo_root)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .context("running cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("parsing cargo metadata")
}

/// Fails naming every missing or extra member and every forbidden edge, over
/// normal, dev and build dependencies alike.
pub fn check_deps(metadata: &Value) -> Result<()> {
    let packages = metadata["packages"]
        .as_array()
        .context("cargo metadata has no packages")?;
    let names: BTreeSet<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    let expected: BTreeSet<&str> = MEMBERS.iter().copied().collect();
    let mut problems = Vec::new();
    for missing in expected.difference(&names) {
        problems.push(format!("missing workspace member {missing}"));
    }
    for extra in names.difference(&expected) {
        problems.push(format!("unexpected workspace member {extra}"));
    }
    for package in packages {
        let from = package["name"].as_str().unwrap_or_default();
        let deps = package["dependencies"].as_array().into_iter().flatten();
        let internal = deps
            .filter_map(|dep| dep["name"].as_str())
            .filter(|to| names.contains(to));
        for to in internal.collect::<BTreeSet<_>>() {
            if !allowed(from, to) {
                problems.push(format!("forbidden dependency edge {from} -> {to}"));
            }
        }
    }
    if !problems.is_empty() {
        bail!(
            "workspace dependency check failed:\n  {}",
            problems.join("\n  ")
        );
    }
    Ok(())
}
