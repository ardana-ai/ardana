//! `cargo xtask build`: the playground's `dist/` through trunk, then the release
//! `ardana` that embeds it. Trunk runs here and not in a build script, which would
//! deadlock on cargo's lock (rust-lang/cargo#8938).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::fetch::lockfile_version;
use crate::sandbox::Sandbox;
use crate::serve::build_ardana;

/// Builds `dist/` and the release `ardana` serving it from memory; returns the binary.
pub fn build(sandbox: &Sandbox) -> Result<PathBuf> {
    build_dist(sandbox)?;
    let ardana = build_ardana(sandbox)?;
    println!("build: {}", ardana.display());
    Ok(ardana)
}

/// Runs the fetched trunk offline, so it takes wasm-bindgen and wasm-opt from
/// `tmp/bin` at the versions `Trunk.toml` pins and never downloads them.
fn build_dist(sandbox: &Sandbox) -> Result<()> {
    let trunk = sandbox.bin_dir().join("trunk");
    if !trunk.is_file() {
        bail!("{} is missing; run `cargo xtask fetch`", trunk.display());
    }
    let crate_dir = sandbox.repo_root().join("crates/ardana-playground");
    check_wasm_bindgen_pin(sandbox, &crate_dir.join("Trunk.toml"))?;
    let status = sandbox
        .command(&trunk)
        .current_dir(&crate_dir)
        .args(["build", "--release", "--offline"])
        .status()
        .context("running trunk")?;
    if !status.success() {
        bail!("trunk build failed ({status})");
    }
    let index = crate_dir.join("dist/index.html");
    if !index.is_file() {
        bail!("trunk build produced no {}", index.display());
    }
    println!("build: {}", crate_dir.join("dist").display());
    Ok(())
}

/// `Trunk.toml` must pin the wasm-bindgen that `Cargo.lock` resolves, which is
/// the wasm-bindgen-cli `cargo xtask fetch` installs.
fn check_wasm_bindgen_pin(sandbox: &Sandbox, trunk_toml: &Path) -> Result<()> {
    let text = std::fs::read_to_string(trunk_toml)
        .with_context(|| format!("reading {}", trunk_toml.display()))?;
    let config: toml::Table = toml::from_str(&text)?;
    let pinned = config
        .get("tools")
        .and_then(|tools| tools.get("wasm_bindgen"))
        .and_then(|version| version.as_str());
    let locked = lockfile_version(sandbox.repo_root(), "wasm-bindgen")?;
    if pinned != Some(locked.as_str()) {
        bail!(
            "{} pins wasm_bindgen {pinned:?}, but Cargo.lock has {locked}",
            trunk_toml.display()
        );
    }
    Ok(())
}
