//! `cargo xtask build`: the playground's `dist/` through trunk, then the release
//! `ardana` that embeds it. `cargo xtask build-playground`: the standalone
//! playground, which no server serves, into `tmp/playground/dist`. Trunk runs here
//! and not in a build script, which would deadlock on cargo's lock
//! (rust-lang/cargo#8938).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::fetch::lockfile_version;
use crate::sandbox::Sandbox;
use crate::serve::{build_ardana, target_dir};

/// The Hugging Face the standalone playground fetches the browser files from, unless `--hub` names a stand-in.
pub const HUB: &str = "https://huggingface.co";
/// The URL the standalone playground reads the library document (C1) from, unless `--library` names another.
pub const LIBRARY: &str = "/models.json";

/// Builds `dist/` and the release `ardana` serving it from memory; returns the binary.
pub fn build(sandbox: &Sandbox) -> Result<PathBuf> {
    let crate_dir = sandbox.repo_root().join("crates/ardana-playground");
    trunk_build(sandbox, &crate_dir.join("dist"), &[], &[])?;
    let ardana = build_ardana(sandbox)?;
    println!("build: {}", ardana.display());
    Ok(ardana)
}

/// Builds the standalone playground into `tmp/playground/dist` and returns that directory: served as static files at
/// `public_url`, it reads the library document from `library` on load and when its tab comes back (Q13), runs the
/// browser variants in the tab from `hub`'s repositories at the commits the document pins, and runs every other model
/// with the ardana CLI. No library is baked: the build reads no snapshot and no hub cache.
/// `crates/ardana-playground/dist`, which `ardana` embeds, stays as it is.
pub fn build_standalone(
    sandbox: &Sandbox,
    public_url: &str,
    hub: &str,
    library: &str,
) -> Result<PathBuf> {
    let dist = sandbox.tmp().join("playground/dist");
    let build = Standalone::new(public_url, hub, library);
    trunk_build(sandbox, &dist, &build.args(), &build.env())?;
    Ok(dist)
}

/// What the standalone build hands trunk beyond the embedded build: the page's `standalone` feature and public URL,
/// and the two URLs the page reads at run time, as compile-time variables of `crates/ardana-playground/src/api.rs`.
struct Standalone {
    public_url: String,
    hub: String,
    library: String,
}

impl Standalone {
    fn new(public_url: &str, hub: &str, library: &str) -> Standalone {
        Standalone {
            public_url: public_url.to_string(),
            hub: hub.trim_end_matches('/').to_string(),
            library: library.to_string(),
        }
    }

    fn args(&self) -> [&str; 4] {
        ["--public-url", &self.public_url, "--features", "standalone"]
    }

    fn env(&self) -> [(&str, &str); 2] {
        [
            ("ARDANA_PLAYGROUND_HUB_URL", &self.hub),
            ("ARDANA_PLAYGROUND_MODELS_URL", &self.library),
        ]
    }
}

/// Runs the fetched trunk offline, so it takes wasm-bindgen and wasm-opt from `tmp/bin` at the versions `Trunk.toml`
/// pins and never downloads them, building the playground into `dist` with `args` and `env` added.
fn trunk_build(sandbox: &Sandbox, dist: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let trunk = sandbox.bin_dir().join("trunk");
    if !trunk.is_file() {
        bail!("{} is missing; run `cargo xtask fetch`", trunk.display());
    }
    let crate_dir = sandbox.repo_root().join("crates/ardana-playground");
    check_wasm_bindgen_pin(sandbox, &crate_dir.join("Trunk.toml"))?;
    // wasm-bindgen never clears the `snippets/` it writes beside its output, and trunk copies that directory whole into
    // every Rust link's target path. The playground's crates have none (`engine.js` is a file of its own), so one an
    // older build left is removed, or `dist/` would carry it.
    let snippets = target_dir(sandbox).join("wasm-bindgen/release/snippets");
    if snippets.exists() {
        std::fs::remove_dir_all(&snippets)
            .with_context(|| format!("removing {}", snippets.display()))?;
    }
    let status = sandbox
        .command(&trunk)
        .current_dir(&crate_dir)
        .args(["build", "--release", "--offline", "--dist"])
        .arg(dist)
        .args(args)
        .envs(env.iter().copied())
        .status()
        .context("running trunk")?;
    if !status.success() {
        bail!("trunk build failed ({status})");
    }
    let index = dist.join("index.html");
    if !index.is_file() {
        bail!("trunk build produced no {}", index.display());
    }
    println!("build: {}", dist.display());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// R6.1: the standalone build bakes no library. Trunk gets the page's feature, its public URL and the two URLs the
    /// page reads at run time, and nothing else: no document, no file of the hub cache, no snapshot (`cargo xtask e2e
    /// standalone` checks the dist it builds names no repository of the snapshot, and that no `library.json` exists).
    #[test]
    fn the_standalone_build_bakes_no_library() {
        let build = Standalone::new("/playground/", "http://127.0.0.1:9/", "/models.json");
        assert_eq!(
            build.args(),
            ["--public-url", "/playground/", "--features", "standalone"]
        );
        assert_eq!(
            build.env(),
            [
                ("ARDANA_PLAYGROUND_HUB_URL", "http://127.0.0.1:9"),
                ("ARDANA_PLAYGROUND_MODELS_URL", "/models.json"),
            ]
        );
        let defaults = Standalone::new("/", HUB, LIBRARY);
        assert_eq!(
            defaults.env(),
            [
                ("ARDANA_PLAYGROUND_HUB_URL", "https://huggingface.co"),
                ("ARDANA_PLAYGROUND_MODELS_URL", "/models.json"),
            ]
        );
    }
}
