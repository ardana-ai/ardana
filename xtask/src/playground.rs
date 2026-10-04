//! `cargo xtask build`: the playground's `dist/` through trunk, then the release
//! `ardana` that embeds it. `cargo xtask build-playground`: the standalone
//! playground, which no server serves, into `tmp/playground/dist`. Trunk runs here
//! and not in a build script, which would deadlock on cargo's lock
//! (rust-lang/cargo#8938).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ardana_api::{BrowserVariant, ModelInfo, ModelsResponse, StandaloneLibrary};
use ardana_registry::BrowserCache;
use ardana_registry::library::library;

use crate::fetch::lockfile_version;
use crate::sandbox::Sandbox;
use crate::serve::{build_ardana, target_dir};

/// The Hugging Face the standalone playground fetches the browser files from, unless `--hub` names a stand-in.
pub const HUB: &str = "https://huggingface.co";

/// Builds `dist/` and the release `ardana` serving it from memory; returns the binary.
pub fn build(sandbox: &Sandbox) -> Result<PathBuf> {
    let crate_dir = sandbox.repo_root().join("crates/ardana-playground");
    trunk_build(sandbox, &crate_dir.join("dist"), &[], &[])?;
    let ardana = build_ardana(sandbox)?;
    println!("build: {}", ardana.display());
    Ok(ardana)
}

/// Builds the standalone playground into `tmp/playground/dist` and returns that directory: served as static files at
/// `public_url`, it lists the library's models, runs their browser variants in the tab from `hub`'s ardana-ai
/// repositories at the commits `tmp/hf/hub` holds, and runs every other model with the ardana CLI.
/// `crates/ardana-playground/dist`, which `ardana` embeds, stays as it is.
pub fn build_standalone(sandbox: &Sandbox, public_url: &str, hub: &str) -> Result<PathBuf> {
    let dir = sandbox.tmp().join("playground");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let library = standalone_library(&sandbox.tmp().join("hf/hub"), hub)?;
    let baked = dir.join("library.json");
    std::fs::write(&baked, serde_json::to_string_pretty(&library)?)
        .with_context(|| format!("writing {}", baked.display()))?;
    let dist = dir.join("dist");
    trunk_build(
        sandbox,
        &dist,
        &["--public-url", public_url, "--features", "standalone"],
        &[("ARDANA_PLAYGROUND_LIBRARY", &baked)],
    )?;
    Ok(dist)
}

/// What the standalone playground bakes in place of a server, from the hub cache at `cache` alone: the library's models
/// as a server that has pulled none lists them, but with no default model and each browser variant held (the Hub
/// holds it whole), and for each browser variant its repository, the commit `refs/main` names in the cache and the
/// profile a server would send for it. A variant the cache does not hold whole fails, naming its model.
pub fn standalone_library(cache: &Path, hub: &str) -> Result<StandaloneLibrary> {
    let held = BrowserCache::at(cache)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .context("starting a runtime for the hub cache")?;
    let mut browser = BTreeMap::new();
    let mut models = Vec::new();
    for model in &library().models {
        if model.browser.is_some() {
            let variant = runtime
                .block_on(held.variant(&model.name))
                .with_context(|| {
                    format!(
                        "the browser variant of {} is not whole in {}; run `cargo xtask fetch`",
                        model.name,
                        cache.display()
                    )
                })?;
            let repository = variant
                .reference
                .strip_prefix(ardana_registry::refs::HF_PREFIX)
                .unwrap_or(&variant.reference)
                .to_string();
            browser.insert(
                model.name.clone(),
                BrowserVariant {
                    repository,
                    commit: variant.commit,
                    profile: serde_json::to_value(&variant.profile)?,
                },
            );
        }
        let pick = library()
            .find(&model.name)
            .with_context(|| format!("the library does not resolve its own {}", model.name))?;
        models.push(ModelInfo {
            name: pick.name(),
            description: pick.reference(),
            release_date: model.release_date.clone(),
            x_pulled: Some(false),
            x_default: false,
            x_size: pick.size(),
            x_browser: library().browser_size(&model.name),
            x_browser_pulled: model.browser.is_some(),
            x_browser_default: library().is_browser_default(&model.name),
        });
    }
    Ok(StandaloneLibrary {
        hub: hub.trim_end_matches('/').to_string(),
        models: ModelsResponse { models },
        browser,
    })
}

/// Runs the fetched trunk offline, so it takes wasm-bindgen and wasm-opt from `tmp/bin` at the versions `Trunk.toml`
/// pins and never downloads them, building the playground into `dist` with `args` and `env` added.
fn trunk_build(sandbox: &Sandbox, dist: &Path, args: &[&str], env: &[(&str, &Path)]) -> Result<()> {
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

    /// R2.1: a hub cache without the browser variants builds nothing, and says which model's files are missing.
    #[test]
    fn build_playground_needs_snapshots() {
        let empty = std::env::temp_dir().join(format!("build-playground-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let err = format!("{:#}", standalone_library(&empty, HUB).unwrap_err());
        std::fs::remove_dir_all(&empty).unwrap();
        let first = library()
            .models
            .iter()
            .find(|m| m.browser.is_some())
            .unwrap();
        assert!(
            err.starts_with(&format!(
                "the browser variant of {} is not whole in ",
                first.name
            )),
            "{err}"
        );
        assert!(err.contains("run `cargo xtask fetch`"), "{err}");
    }
}
