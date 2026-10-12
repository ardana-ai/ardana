//! `cargo xtask export-decider`: exports decider's prompt layout cases for
//! `crates/ardana-core/tests/prompt.rs` (R2.2) by running decider@23579f7 from
//! `tmp/src/decider` in a uv venv under `tmp/py/decider-export`.
//!
//! `cargo xtask export-thx01`: exports the `thx01` 1.0.0 package's answers to the request fixtures as the goldens
//! of `crates/ardana-core/tests/data/thx01/` (R8.2), in a uv venv under `tmp/py/thx01-export` on the CPU.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::fetch::{Hf, Manifest};
use crate::python::pinned_venv;
use crate::sandbox::{Sandbox, run};

/// The venv's Python and packages; decider 1.6.0 itself runs from the clone.
const PYTHON: &str = "3.12";
const PACKAGES: &[&str] = &[
    "torch==2.14.0",
    "transformers==5.17.0",
    "numpy==1.26.4",
    "fastapi==0.141.1",
    "jinja2==3.1.6",
    "huggingface-hub==1.33.0",
];
const TOKENIZER_REPO: &str = "Mapika/decider-2b-GGUF";
const OUT_DIR: &str = "crates/ardana-core/tests/data/decider";

/// The `thx01` export's packages: the package itself and the CPU build of the torch it runs on, from PyTorch's CPU
/// index (the PyPI wheel carries CUDA), with the transformers, numpy, safetensors and huggingface-hub it imports.
const THX01_PACKAGES: &[&str] = &[
    "thx01==1.0.0",
    "torch==2.14.0+cpu",
    "transformers==5.17.0",
    "numpy==1.26.4",
    "safetensors==0.8.0",
    "huggingface-hub==1.33.0",
];
const TORCH_CPU_INDEX: &str = "https://download.pytorch.org/whl/cpu";
/// The checkpoint the goldens come from: `doofz/THX-01` at the commit the `thx-01` row of
/// `crate::e2e::CANDLE_FIXTURES` pins.
const THX01_REPO: &str = "doofz/THX-01";
const THX01_COMMIT: &str = "bef2e2478be7acd79429324a658558a57552a2e8";
const THX01_OUT_DIR: &str = "crates/ardana-core/tests/data/thx01";
const REQUESTS_DIR: &str = "tests/fixtures/requests";

pub fn export_decider(sandbox: &Sandbox) -> Result<()> {
    let root = sandbox.repo_root();
    let manifest = Manifest::load(root)?;
    let repo: &Hf = manifest
        .hf
        .iter()
        .find(|hf| hf.repo == TOKENIZER_REPO)
        .with_context(|| format!("xtask/fetch.toml has no [[hf]] entry for {TOKENIZER_REPO}"))?;
    let tokenizer_dir = repo
        .repo_dir(sandbox)
        .join("snapshots")
        .join(&repo.revision);
    let decider = sandbox.tmp().join("src/decider");
    for path in [
        tokenizer_dir.join("tokenizer.json"),
        decider.join("tests/layout_cases.py"),
    ] {
        if !path.is_file() {
            bail!("{} is missing; run `cargo xtask fetch`", path.display());
        }
    }

    let venv = sandbox.tmp().join("py/decider-export");
    let python = pinned_venv(sandbox, &venv, PYTHON, PACKAGES, None)?;
    run(sandbox
        .command(&python)
        .current_dir(root)
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .arg(root.join("xtask/scripts/decider_layout_cases.py"))
        .arg(&decider)
        .arg(&tokenizer_dir)
        .arg(root.join(OUT_DIR)))
}

/// The checkpoint is downloaded into the scratch directory `tmp/thx01-export` (not the hub cache: the package
/// rewrites `tokenizer/tokenizer_config.json` in place where it loads from, which the cache forbids), the script
/// answers the three fixtures from it, and the directory is removed whether or not the script succeeded, so the
/// 644 MB stay on disk only while the goldens are written. The venv stays.
pub fn export_thx01(sandbox: &Sandbox) -> Result<()> {
    let root = sandbox.repo_root();
    let venv = sandbox.tmp().join("py/thx01-export");
    let python = pinned_venv(
        sandbox,
        &venv,
        PYTHON,
        THX01_PACKAGES,
        Some(TORCH_CPU_INDEX),
    )?;
    let scratch = sandbox.tmp().join("thx01-export");
    remove_if_present(&scratch)?;
    std::fs::create_dir_all(&scratch).with_context(|| format!("creating {}", scratch.display()))?;
    let exported = run(sandbox
        .command(&python)
        .current_dir(root)
        .arg(root.join("xtask/scripts/thx01_goldens.py"))
        .arg(format!("{THX01_REPO}@{THX01_COMMIT}"))
        .arg(root.join(REQUESTS_DIR))
        .arg(&scratch)
        .arg(root.join(THX01_OUT_DIR)));
    remove_if_present(&scratch)?;
    exported
}

fn remove_if_present(dir: &Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    Ok(())
}
