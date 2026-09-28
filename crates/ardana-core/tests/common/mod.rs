//! Shared helpers: the Hub files `cargo xtask fetch` put into `tmp/hf`, and Python-style digests.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::path::PathBuf;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

pub const DECIDER_2B: &str = "Mapika/decider-2b-GGUF";

/// The pinned snapshot directory of `repo` (`org/name`) in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
pub fn hf_snapshot(repo: &str) -> Result<PathBuf> {
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
    Ok(dir.join("snapshots").join(rev.trim()))
}

/// A file of the pinned snapshot of `repo` in `tmp/hf`.
pub fn hf_file(repo: &str, name: &str) -> Result<PathBuf> {
    let path = hf_snapshot(repo)?.join(name);
    anyhow::ensure!(
        path.is_file(),
        "{} is missing; run `cargo xtask fetch`",
        path.display()
    );
    Ok(path)
}

/// A file of the pinned decider-2b-GGUF snapshot.
pub fn decider_2b_file(name: &str) -> Result<PathBuf> {
    hf_file(DECIDER_2B, name)
}

/// The `tokenizer.json` of the pinned snapshot of `repo`.
pub fn tokenizer(repo: &str) -> Result<Tokenizer> {
    let path = hf_file(repo, "tokenizer.json")?;
    Tokenizer::from_file(&path).map_err(|err| anyhow::anyhow!("loading {}: {err}", path.display()))
}

pub fn decider_2b_tokenizer() -> Result<Tokenizer> {
    tokenizer(DECIDER_2B)
}

/// `hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False).encode()).hexdigest()`.
pub fn digest(value: &serde_json::Value) -> String {
    let text = ardana_core::py::dumps(value, true);
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The vendored decider test data.
pub fn decider_data(name: &str) -> Result<serde_json::Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/decider")
        .join(name);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}
