//! `cargo xtask export-decider`: exports decider's prompt layout cases for
//! `crates/ardana-core/tests/prompt.rs` (R2.2) by running decider@23579f7 from
//! `tmp/src/decider` in a uv venv under `tmp/py/decider-export`.

use anyhow::{Context, Result, bail};

use crate::fetch::{Hf, Manifest};
use crate::python::{managed_venv, uv};
use crate::sandbox::Sandbox;

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
    let python = venv.join("bin/python");
    if !managed_venv(sandbox, &venv) {
        run(uv(sandbox)
            .args(["venv", "--quiet", "--clear", "--python", PYTHON])
            .arg(&venv))?;
    }
    run(uv(sandbox)
        .args(["pip", "install", "--quiet", "--python"])
        .arg(&python)
        .args(PACKAGES))?;
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

fn run(cmd: &mut std::process::Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("{:?} failed ({status})", cmd.get_program());
    }
    Ok(())
}
