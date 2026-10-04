//! R3.5 to R3.7: `ardana run --layout chat` reads stock instruct GGUFs zero-shot, with the chat template next to the
//! official tokenizer, on the sentiment fixture.

mod common;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{Value, json};

const QWEN35_GGUF: (&str, &str) = ("ggml-org/Qwen3.5-0.8B-GGUF", "Qwen3.5-0.8B-Q4_0.gguf");
const QWEN35_TOKENIZER: &str = "Qwen/Qwen3.5-0.8B";
const SMOLLM3_GGUF: (&str, &str) = ("ggml-org/SmolLM3-3B-GGUF", "SmolLM3-Q4_K_M.gguf");
const SMOLLM3_TOKENIZER: &str = "HuggingFaceTB/SmolLM3-3B";
const GEMMA4_GGUF: (&str, &str) = ("ggml-org/gemma-4-E2B-it-GGUF", "gemma-4-E2B-it-Q4_0.gguf");
const GEMMA4_TOKENIZER: &str = "google/gemma-4-E2B-it";

/// `ardana run --layout chat` without `--config` on a GGUF and the tokenizer repo it was converted from.
fn run_chat(gguf: (&str, &str), tokenizer: &str, request: PathBuf) -> Result<Value> {
    let args: Vec<OsString> = vec![
        "--gguf".into(),
        common::hf_file(gguf.0, gguf.1)?.into(),
        "--tokenizer".into(),
        common::hf_file(tokenizer, "tokenizer.json")?.into(),
        "--layout".into(),
        "chat".into(),
        "--request".into(),
        request.into(),
    ];
    common::ardana_run(args)
}

#[test]
#[ignore = "e2e: ggml-org/Qwen3.5-0.8B-GGUF Q4_0 and the Qwen/Qwen3.5-0.8B tokenizer in tmp/hf (cargo xtask fetch)"]
fn qwen35_0_8b() -> Result<()> {
    let resp = run_chat(
        QWEN35_GGUF,
        QWEN35_TOKENIZER,
        common::request("sentiment.json"),
    )?;
    common::check_sentiment(&resp)?;
    Ok(())
}

#[test]
#[ignore = "e2e: ggml-org/SmolLM3-3B-GGUF Q4_K_M and the HuggingFaceTB/SmolLM3-3B tokenizer in tmp/hf (cargo xtask fetch)"]
fn smollm3_3b() -> Result<()> {
    let resp = run_chat(
        SMOLLM3_GGUF,
        SMOLLM3_TOKENIZER,
        common::request("sentiment.json"),
    )?;
    common::check_sentiment(&resp)?;
    Ok(())
}

#[test]
#[ignore = "e2e: ggml-org/gemma-4-E2B-it-GGUF Q4_0 and the google/gemma-4-E2B-it tokenizer in tmp/hf (cargo xtask fetch)"]
fn gemma4_e2b() -> Result<()> {
    let resp = run_chat(
        GEMMA4_GGUF,
        GEMMA4_TOKENIZER,
        common::request("sentiment.json"),
    )?;
    common::check_sentiment(&resp)?;
    Ok(())
}

/// Without `--config` the model gets `ModelProfile::stock`: listwise score levels, so no `x_level_fit`.
#[test]
#[ignore = "e2e: ggml-org/Qwen3.5-0.8B-GGUF Q4_0 and the Qwen/Qwen3.5-0.8B tokenizer in tmp/hf (cargo xtask fetch)"]
fn stock_profile() -> Result<()> {
    let tmp = PathBuf::from(std::env::var_os("ARDANA_TMP").context("ARDANA_TMP is not set")?);
    std::fs::create_dir_all(&tmp)?;
    let request = tmp.join("e2e-chat-score.json");
    let body = json!({
        "model": "jev-latest",
        "state": "I absolutely love this product, it works perfectly.",
        "questions": {
            "satisfaction": {
                "type": "score",
                "instructions": "How satisfied is the customer?",
                "criteria": ["very unhappy", "unhappy", "neutral", "happy", "very happy"]
            }
        }
    });
    std::fs::write(&request, serde_json::to_string(&body)?)?;
    let resp = run_chat(QWEN35_GGUF, QWEN35_TOKENIZER, request)?;
    assert_eq!(resp["model"], "decider-dev");
    let score = &resp["answers"]["satisfaction"];
    assert_eq!(score["type"], "score", "{score}");
    let fields = score.as_object().context("score answer")?;
    assert!(!fields.contains_key("x_level_fit"), "{score}");
    assert!(!fields.contains_key("x_fit_mass"), "{score}");
    Ok(())
}
