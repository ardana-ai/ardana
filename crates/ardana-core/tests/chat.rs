//! R3.1 to R3.4: chat-layout head and tail ids derived from the chat templates of the official tokenizers in `tmp/hf`,
//! and the start-up checks, as decider 1.6.0's `ChatTemplate` derives and runs them.

mod common;

use anyhow::{Context, Result, ensure};
use ardana_core::chat::{self, TemplateSpecials};
use ardana_core::{Layout, prompt};
use tokenizers::{AddedToken, Tokenizer};

const QWEN35: &str = "Qwen/Qwen3.5-0.8B";
const SMOLLM3: &str = "HuggingFaceTB/SmolLM3-3B";
const LLAMA32: &str = "meta-llama/Llama-3.2-3B-Instruct";

/// The chat layout of the tokenizer and template files of `repo`'s pinned snapshot.
fn layout(repo: &str) -> Result<(Tokenizer, Vec<u32>, Vec<u32>)> {
    let tok = common::tokenizer(repo)?;
    let (template, specials) = chat::read_template(&common::hf_snapshot(repo)?)?;
    let Layout::Chat { head, tail } = chat::chat_layout(&template, &tok, &specials)? else {
        anyhow::bail!("chat_layout returned the plain layout");
    };
    Ok((tok, head, tail))
}

fn decode(tok: &Tokenizer, ids: &[u32]) -> Result<String> {
    tok.decode(ids, false).map_err(anyhow::Error::msg)
}

/// decider `tests/test_layout.py::test_chat_template_is_the_served_one` on Qwen/Qwen3.5-0.8B's template.
#[test]
fn qwen35_head_tail() -> Result<()> {
    let (tok, head, tail) = layout(QWEN35)?;
    assert_eq!(head, [248045, 846, 198]);
    assert_eq!(
        tail,
        [248046, 198, 248045, 74455, 198, 248068, 271, 248069, 271]
    );
    assert_eq!(decode(&tok, &head)?, "<|im_start|>user\n");
    assert_eq!(
        decode(&tok, &tail)?,
        "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
    );
    Ok(())
}

/// SmolLM3's template (with its `{% generation %}` block and `enable_thinking` switch) renders in no-think mode and
/// passes both start-up checks; the tail is the one decider derives through transformers.
#[test]
fn smollm3_layout() -> Result<()> {
    let (tok, head, tail) = layout(SMOLLM3)?;
    assert_eq!(
        tail,
        [128012, 198, 128011, 78191, 198, 128002, 271, 128003, 198]
    );
    assert_eq!(
        decode(&tok, &tail)?,
        "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n"
    );
    let head = decode(&tok, &head)?;
    assert!(
        head.starts_with("<|im_start|>system\n## Metadata\n\n"),
        "{head}"
    );
    assert!(head.contains("Reasoning Mode: /no_think\n"), "{head}");
    assert!(head.contains("Today Date: 26 July 2024\n"), "{head}");
    assert!(head.ends_with("<|im_start|>user\n"), "{head}");
    Ok(())
}

/// Llama 3.2's template prints `bos_token` once and takes its date from `strftime_now`, which formats the fixed
/// template date, so the head is the same on every day.
#[test]
#[ignore = "e2e: the gated Llama 3.2 tokenizer, copied into tmp/hf from the local Hub cache (cargo xtask fetch)"]
fn llama32_layout() -> Result<()> {
    let (tok, head, tail) = layout(LLAMA32)?;
    let want_head = "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\nCutting Knowledge Date: December 2023\nToday Date: 26 Jul 2024\n\n<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n";
    assert_eq!(decode(&tok, &head)?, want_head);
    let bos = tok
        .token_to_id("<|begin_of_text|>")
        .context("no <|begin_of_text|>")?;
    assert_eq!(head.iter().filter(|&&id| id == bos).count(), 1);
    assert_eq!(head[0], bos);
    assert_eq!(
        decode(&tok, &tail)?,
        "<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n"
    );

    // The date comes through `strftime_now`: with the template's own fallback date changed, the head is unchanged.
    let (template, specials) = chat::read_template(&common::hf_snapshot(LLAMA32)?)?;
    let fallback = "{%- set date_string = \"26 Jul 2024\" %}";
    ensure!(
        template.contains("strftime_now is defined") && template.contains(fallback),
        "the Llama 3.2 template no longer probes strftime_now"
    );
    let changed = template.replace(fallback, "{%- set date_string = \"fallback\" %}");
    assert_eq!(
        chat::chat_layout(&changed, &tok, &specials)?,
        Layout::Chat { head, tail }
    );
    Ok(())
}

/// A fresh directory under `$ARDANA_TMP` for one test.
fn scratch(name: &str) -> Result<std::path::PathBuf> {
    let root = std::env::var_os("ARDANA_TMP").context("ARDANA_TMP is not set")?;
    let dir = std::path::PathBuf::from(root).join("chat-tests").join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn chat_errors() -> Result<()> {
    // Neither template source: an empty directory, and a tokenizer_config.json without "chat_template".
    let empty = scratch("empty")?;
    let err = chat::read_template(&empty).unwrap_err().to_string();
    assert!(err.contains("has no chat template"), "{err}");
    let no_template = scratch("no-template")?;
    std::fs::write(
        no_template.join("tokenizer_config.json"),
        r#"{"bos_token": null, "eos_token": "<|im_end|>"}"#,
    )?;
    let err = chat::read_template(&no_template).unwrap_err().to_string();
    assert!(err.contains("has no chat template"), "{err}");

    let tok = common::tokenizer(QWEN35)?;
    let specials = TemplateSpecials::default();

    // A tail ending in a space merges with "Answer" into " Answer".
    let merging = "{{ messages[0].content }} ";
    let err = chat::chat_layout(merging, &tok, &specials)
        .unwrap_err()
        .to_string();
    assert!(err.contains("tail_boundary"), "{err}");

    // A tokenizer that reads "(A" as one token breaks the label after "Answer: (".
    let mut merged = tok.clone();
    merged
        .add_tokens([AddedToken::from("(A", false)])
        .map_err(anyhow::Error::msg)?;
    assert_eq!(prompt::encode(&merged, "A")?.len(), 1);
    let err = chat::chat_layout("{{ messages[0].content }}", &merged, &specials)
        .unwrap_err()
        .to_string();
    assert!(err.contains("labels_single_token"), "{err}");
    assert!(err.contains("\"A\""), "{err}");
    Ok(())
}
