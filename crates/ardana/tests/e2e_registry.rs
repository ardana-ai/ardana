//! R4.1 to R4.3 and R4.7: `ardana pull` records `hf.co/` and `ollama:` models in `$ARDANA_HOME/models.toml` from
//! the files `cargo xtask fetch` put into `tmp/hf` (offline) and the local Ollama store, and `ardana run <name>`
//! answers with each of them.

mod common;

use std::path::PathBuf;

use anyhow::{Context, Result};
use common::Ardana;
use toml::Value;

const DECIDER_2B: &str = "Mapika/decider-2b-GGUF";
const QWEN35_PULL: [&str; 6] = [
    "pull",
    "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0",
    "--tokenizer",
    "hf.co/Qwen/Qwen3.5-0.8B",
    "--name",
    "qwen3.5-0.8b",
];
const SMOLLM3_PULL: [&str; 6] = [
    "pull",
    "hf.co/ggml-org/SmolLM3-3B-GGUF:Q4_K_M",
    "--tokenizer",
    "hf.co/HuggingFaceTB/SmolLM3-3B",
    "--name",
    "smollm3-3b",
];
const LLAMA32_PULL: [&str; 4] = [
    "pull",
    "ollama:llama3.2",
    "--tokenizer",
    "hf.co/meta-llama/Llama-3.2-3B-Instruct",
];
/// The size of Ollama's `llama3.2:latest` model blob.
const LLAMA32_BLOB_BYTES: u64 = 2_019_377_376;

fn str_of<'a>(table: &'a toml::Table, key: &str) -> Result<&'a str> {
    table
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("{key} in {table:?}"))
}

fn path_of(table: &toml::Table, key: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(str_of(table, key)?))
}

fn profile(entry: &toml::Table) -> Result<&toml::Table> {
    entry
        .get("profile")
        .and_then(Value::as_table)
        .context("profile")
}

fn layout_kind(entry: &toml::Table) -> Result<&str> {
    let layout = profile(entry)?
        .get("layout")
        .and_then(Value::as_table)
        .context("layout")?;
    str_of(layout, "kind")
}

/// `pulled_at` is a `YYYY-MM-DD` date.
fn check_pulled_at(entry: &toml::Table) -> Result<()> {
    let date = str_of(entry, "pulled_at")?;
    let parts: Vec<&str> = date.split('-').collect();
    assert!(
        matches!(parts[..], [y, m, d] if y.len() == 4 && m.len() == 2 && d.len() == 2
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))),
        "pulled_at {date}"
    );
    Ok(())
}

/// A stock chat entry: `ModelProfile::stock` named after the entry, in the chat layout.
fn check_stock_chat(entry: &toml::Table, name: &str) -> Result<()> {
    let profile = profile(entry)?;
    assert_eq!(str_of(profile, "name")?, name);
    assert_eq!(layout_kind(entry)?, "chat");
    assert_eq!(
        profile.get("temperature").and_then(Value::as_float),
        Some(1.0)
    );
    assert_eq!(
        profile.get("isolated_levels").and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        profile
            .get("temperature_by_type")
            .and_then(Value::as_table)
            .is_none_or(toml::Table::is_empty)
    );
    assert!(profile.get("release_date").is_none());
    check_pulled_at(entry)
}

/// The Ollama store `ardana` reads: `$OLLAMA_MODELS`, else `~/.ollama/models`.
fn ollama_models() -> Result<PathBuf> {
    match std::env::var_os("OLLAMA_MODELS") {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(PathBuf::from(std::env::var_os("HOME").context("HOME")?).join(".ollama/models")),
    }
}

#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF and files in tmp/hf (cargo xtask fetch)"]
fn pull_decider() -> Result<()> {
    let ardana = Ardana::new("e2e-registry-pull-decider")?;
    // The endpoint is unreachable: with HF_HUB_OFFLINE=1 the pull never calls it.
    let pull = |args: &[&str]| -> Result<()> {
        let mut cmd = ardana.command(args);
        cmd.env("HF_ENDPOINT", "http://127.0.0.1:9");
        let output = cmd.output()?;
        println!(
            "{cmd:?}:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{cmd:?} failed");
        Ok(())
    };
    pull(&["pull", "hf.co/Mapika/decider-2b-GGUF:q4_k_m"])?;
    let entry = ardana.entry("decider-2b")?;
    assert_eq!(
        str_of(&entry, "source")?,
        "hf.co/Mapika/decider-2b-GGUF:q4_k_m"
    );
    assert_eq!(
        path_of(&entry, "weights")?,
        common::hf_file(DECIDER_2B, "decider-2b-v11-Q4_K_M.gguf")?
    );
    assert_eq!(
        path_of(&entry, "tokenizer")?,
        common::hf_file(DECIDER_2B, "tokenizer.json")?
    );
    assert_eq!(str_of(&entry, "runtime")?, "llama.cpp");
    check_pulled_at(&entry)?;
    // decider_config.json: version 2b-v11, release 2026-09-24, plain layout, isolated levels, fitted temperatures.
    let profile = profile(&entry)?;
    assert_eq!(str_of(profile, "name")?, "decider-2b-v11");
    assert_eq!(str_of(profile, "release_date")?, "2026-09-24");
    assert_eq!(layout_kind(&entry)?, "plain");
    assert_eq!(
        profile.get("isolated_levels").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        profile.get("temperature").and_then(Value::as_float),
        Some(1.145)
    );
    let by_type = profile
        .get("temperature_by_type")
        .and_then(Value::as_table)
        .context("temperature_by_type")?;
    assert_eq!(by_type.get("noul").and_then(Value::as_float), Some(1.624));

    // `--name` overrides the derived name; the first entry stays.
    pull(&["pull", "hf.co/Mapika/decider-2b-GGUF", "--name", "d2b"])?;
    let renamed = ardana.entry("d2b")?;
    assert_eq!(
        path_of(&renamed, "weights")?,
        path_of(&entry, "weights")?,
        "without a quant the pull picks Q4_K_M"
    );
    ardana.entry("decider-2b")?;
    Ok(())
}

#[test]
#[ignore = "e2e: Qwen3.5-0.8B and SmolLM3-3B GGUFs and tokenizers in tmp/hf (cargo xtask fetch)"]
fn pull_stock() -> Result<()> {
    let ardana = Ardana::new("e2e-registry-pull-stock")?;
    ardana.ok(QWEN35_PULL)?;
    let qwen = ardana.entry("qwen3.5-0.8b")?;
    assert_eq!(
        path_of(&qwen, "weights")?,
        common::hf_file("ggml-org/Qwen3.5-0.8B-GGUF", "Qwen3.5-0.8B-Q4_0.gguf")?
    );
    assert_eq!(
        path_of(&qwen, "tokenizer")?,
        common::hf_file("Qwen/Qwen3.5-0.8B", "tokenizer.json")?
    );
    check_stock_chat(&qwen, "qwen3.5-0.8b")?;
    // R3.1's head, from the chat template next to the tokenizer.
    let head = profile(&qwen)?["layout"]["head"].clone();
    assert_eq!(head, Value::try_from(vec![248_045, 846, 198])?);

    ardana.ok(SMOLLM3_PULL)?;
    let smollm = ardana.entry("smollm3-3b")?;
    assert_eq!(
        path_of(&smollm, "weights")?,
        common::hf_file("ggml-org/SmolLM3-3B-GGUF", "SmolLM3-Q4_K_M.gguf")?
    );
    assert_eq!(
        path_of(&smollm, "tokenizer")?,
        common::hf_file("HuggingFaceTB/SmolLM3-3B", "tokenizer.json")?
    );
    check_stock_chat(&smollm, "smollm3-3b")?;
    Ok(())
}

#[test]
#[ignore = "e2e: Ollama llama3.2 in the local store, Llama 3.2 tokenizer in tmp/hf (cargo xtask fetch)"]
fn pull_ollama() -> Result<()> {
    let store = ollama_models()?;
    let manifest = store.join("manifests/registry.ollama.ai/library/llama3.2/latest");
    assert!(manifest.is_file(), "{} is missing", manifest.display());
    let ardana = Ardana::new("e2e-registry-pull-ollama")?;
    let mut cmd = ardana.command(LLAMA32_PULL);
    cmd.env("OLLAMA_MODELS", &store);
    let output = cmd.output()?;
    assert!(
        output.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entry = ardana.entry("llama3.2")?;
    assert_eq!(str_of(&entry, "source")?, "ollama:llama3.2");
    // The blob is read in place: a sha256 blob of the store, never a copy.
    let weights = path_of(&entry, "weights")?;
    assert_eq!(weights.parent(), Some(store.join("blobs").as_path()));
    assert!(
        weights
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("sha256-"))
    );
    assert_eq!(std::fs::metadata(&weights)?.len(), LLAMA32_BLOB_BYTES);
    let copies = std::fs::read_dir(&ardana.home)?
        .filter_map(Result::ok)
        .filter(|e| e.file_name() != "models.toml")
        .count();
    assert_eq!(copies, 0, "the home holds only models.toml");
    assert_eq!(
        path_of(&entry, "tokenizer")?,
        common::hf_file("meta-llama/Llama-3.2-3B-Instruct", "tokenizer.json")?
    );
    check_stock_chat(&entry, "llama3.2")?;
    // One <|begin_of_text|> (128000) opens the head (R3.3).
    let head = profile(&entry)?["layout"]["head"]
        .as_array()
        .context("head")?
        .clone();
    assert_eq!(head.first(), Some(&Value::Integer(128_000)));
    assert_eq!(
        head.iter()
            .filter(|&id| id == &Value::Integer(128_000))
            .count(),
        1
    );
    Ok(())
}

#[test]
#[ignore = "e2e: decider-2b, Qwen3.5-0.8B, SmolLM3-3B in tmp/hf and Ollama llama3.2, loaded through llama.cpp"]
fn run_by_name() -> Result<()> {
    let ardana = Ardana::new("e2e-registry-run-by-name")?;
    ardana.ok(["pull", "hf.co/Mapika/decider-2b-GGUF:Q4_K_M"])?;
    ardana.ok(QWEN35_PULL)?;
    ardana.ok(SMOLLM3_PULL)?;
    let mut cmd = ardana.command(LLAMA32_PULL);
    cmd.env("OLLAMA_MODELS", ollama_models()?);
    assert!(cmd.status()?.success(), "{cmd:?} failed");
    let list = ardana.ok(["list"])?;
    println!("{list}");

    let ticket = common::request("ticket.json");
    let metal = common::check_ticket(&ardana.run_named("decider-2b", &ticket, &[])?)?;
    let cpu =
        common::check_ticket(&ardana.run_named("decider-2b", &ticket, &["--gpu-layers", "0"])?)?;
    assert_eq!(metal, cpu, "the CPU run has the same argmaxes");

    let sentiment = common::request("sentiment.json");
    for name in ["qwen3.5-0.8b", "smollm3-3b", "llama3.2"] {
        let resp = ardana.run_named(name, &sentiment, &[])?;
        assert_eq!(resp["model"], name, "a stock model reports its entry name");
        common::check_sentiment(&resp).with_context(|| format!("{name}: {resp}"))?;
    }
    Ok(())
}

/// A local GGUF path: read in place, with `decider_config.json` and `tokenizer.json` from beside it.
#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF and files in tmp/hf (cargo xtask fetch)"]
fn pull_local() -> Result<()> {
    let ardana = Ardana::new("e2e-registry-pull-local")?;
    let weights = common::hf_file(DECIDER_2B, "decider-2b-v11-Q4_K_M.gguf")?;
    let mut cmd = ardana.command(["pull"]);
    cmd.arg(&weights);
    assert!(cmd.status()?.success(), "{cmd:?} failed");
    let entry = ardana.entry("decider-2b-v11-q4_k_m")?;
    assert_eq!(path_of(&entry, "weights")?, weights);
    assert_eq!(str_of(&entry, "source")?, weights.to_string_lossy());
    assert_eq!(
        path_of(&entry, "tokenizer")?,
        common::hf_file(DECIDER_2B, "tokenizer.json")?
    );
    assert_eq!(str_of(profile(&entry)?, "name")?, "decider-2b-v11");
    assert_eq!(layout_kind(&entry)?, "plain");
    Ok(())
}
