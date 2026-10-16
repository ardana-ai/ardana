//! `ardana finetune`'s refusals, before anything heavy happens (R1.4, R1.6, R3.3): data that cannot be trained on is
//! named by file, line and question once the base's tokenizer is read, before the training environment and the
//! checkpoint's weights; a name an entry or the library holds is refused before even the base is read; a base whose
//! checkpoint is unknown is refused naming `--checkpoint`, and an entry pulled from a GGUF repository resolves to the
//! checkpoint its card names (R3.4); `--format safetensors` of a checkpoint Candle does not run is refused (R4.2).
//! Every run is offline, on the files `cargo xtask fetch` put into `tmp/hf` (qwen3:1.7b's tokenizer and checkpoint
//! listing, which the library snapshot pins, and its GGUF repository's card; qwen3.5:0.8b's tokenizer and
//! `config.json`).

mod common;

use std::path::Path;

use anyhow::{Context, Result, ensure};
use common::Ardana;
use serde_json::{Value, json};

/// A library size whose tokenizer and checkpoint listing `cargo xtask fetch` installs, without its weights.
const BASE: &str = "qwen3:1.7b";

/// The questions file: the shared questions of every line.
fn questions(dir: &Path) -> Result<std::path::PathBuf> {
    let path = dir.join("questions.json");
    let questions = json!({
        "risk": {"type": "choice", "instructions": "How risky is running this shell command?",
                 "criteria": {"safe": "read-only", "caution": "can be undone", "destructive": "cannot be undone"}},
        "outside": {"type": "noul", "instructions": "Does the command touch anything outside the project?"},
    });
    std::fs::write(&path, questions.to_string())?;
    Ok(path)
}

/// `n` valid lines of the shared questions with `line` inserted at `at`, written to `data.jsonl` in `dir`.
fn data(dir: &Path, n: usize, at: usize, line: &str) -> Result<std::path::PathBuf> {
    let mut lines: Vec<String> = (0..n)
        .map(|i| {
            let risk = ["safe", "caution", "destructive"][i % 3];
            json!({"state": format!("ls -la dir{i}"), "answers": {"risk": risk, "outside": i % 2 == 0}}).to_string()
        })
        .collect();
    lines.insert(at, line.to_string());
    let path = dir.join("data.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n")?;
    Ok(path)
}

/// `ardana finetune <args>` in `ardana`'s home, with the Ollama store `ollama/` beside it, which must exit 1; returns
/// its stderr.
fn refused(ardana: &Ardana, args: &[&str]) -> Result<String> {
    let store = ardana.home.with_file_name("ollama");
    let output = ardana
        .command(std::iter::once("finetune").chain(args.iter().copied()))
        .env("OLLAMA_MODELS", &store)
        .output()?;
    println!(
        "ardana finetune {args:?} ({}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(
        output.status.code() == Some(1),
        "ardana finetune {args:?} exited {}",
        output.status
    );
    let stderr = String::from_utf8(output.stderr)?;
    ensure!(
        !ardana.home.join("finetune").exists(),
        "ardana finetune {args:?} started the training environment before refusing: {stderr}"
    );
    ensure!(
        !ardana.home.join("models").exists(),
        "ardana finetune {args:?} wrote a model directory before refusing: {stderr}"
    );
    Ok(stderr)
}

/// R1.4: each kind of bad data stops the run once the base's tokenizer is read, naming `<file>:<line>` and the
/// question, before the training environment and the checkpoint's weights (which the offline cache lacks: a run that
/// went for them first would fail on them instead); a line without labels is skipped, and under 100 labelled lines
/// are refused.
#[test]
fn refuses_bad_data_before_downloading() -> Result<()> {
    let ardana = Ardana::new("finetune-bad-data")?;
    let dir = ardana.home.parent().context("the home's directory")?;
    let questions = questions(dir)?;
    let questions = questions.to_str().context("a UTF-8 path")?;
    // Each case: where its one line goes among the valid lines, the line, how many valid lines, what is named.
    let cases = [
        (
            4,
            r#"{"state": "rm -rf /", "answers": {"risk": "destructive""#,
            120,
            ":5: invalid JSON",
        ),
        (
            9,
            r#"{"state": "rm -rf /", "answers": {"risky": "destructive"}}"#,
            120,
            ":10: question risky: the line answers a question it does not ask",
        ),
        (
            0,
            r#"{"state": "rm -rf /", "answers": {"risk": "fatal"}}"#,
            120,
            ":1: question risk: 'fatal' is not one of its options (safe, caution, destructive)",
        ),
        (
            119,
            r#"{"state": "rm -rf /", "answers": {"outside": "yes"}}"#,
            120,
            ":120: question outside: a noul label is true or false, got 'yes'",
        ),
        (
            2,
            r#"{"state": "s", "questions": {"pick": {"type": "score", "instructions": "How?", "criteria": ["one"]}}, "answers": {"pick": 0}}"#,
            120,
            ":3: question pick: score criteria: an ordered list of 2..10 level descriptions",
        ),
        (
            3,
            r#"{"state": "s", "answers": {"risk": null, "outside": null}}"#,
            99,
            ": 99 labelled lines; fine-tuning needs at least 100",
        ),
    ];
    for (at, line, n, wanted) in cases {
        let data = data(dir, n, at, line)?;
        let data = data.to_str().context("a UTF-8 path")?;
        let stderr = refused(
            &ardana,
            &[BASE, data, "--questions", questions, "--name", "risky"],
        )?;
        ensure!(
            stderr.contains(&format!("{data}{wanted}")),
            "the refusal names no `{data}{wanted}`:\n{stderr}"
        );
    }
    Ok(())
}

/// R1.6: a `--name` an entry, a library family or a library name holds (in any spelling) is refused with exit 1
/// before the base is read: the data file named here does not even exist.
#[test]
fn refuses_taken_names() -> Result<()> {
    let ardana = Ardana::new("finetune-taken-names")?;
    let dir = ardana.home.parent().context("the home's directory")?;
    // An entry: a file with the GGUF magic, pulled offline with a tokenizer from the cache.
    let gguf = dir.join("mine.gguf");
    std::fs::write(&gguf, b"GGUF\x03\0\0\0")?;
    let gguf = gguf.to_str().context("a UTF-8 path")?;
    ardana.ok([
        "pull",
        gguf,
        "--tokenizer",
        "hf.co/Qwen/Qwen3-1.7B",
        "--name",
        "mine",
    ])?;
    let entry: Value = serde_json::from_str(&ardana.ok(["show", "mine", "--json"])?)?;
    ensure!(entry["name"] == "mine", "{entry}");

    let missing = dir.join("no-such-data.jsonl");
    let missing = missing.to_str().context("a UTF-8 path")?;
    for (name, taken) in [
        ("mine", "taken by a pulled model"),
        ("decider", "taken by the library family decider"),
        ("Qwen3", "taken by the library family qwen3"),
        ("qwen3:1.7b", "taken by the library model qwen3:1.7b"),
        (
            "Decider:2B-Q8_0",
            "taken by the library model decider:2b-q8_0",
        ),
        ("decider:9b", "taken by the library family decider"),
    ] {
        let stderr = refused(&ardana, &[BASE, missing, "--name", name])?;
        ensure!(
            stderr.contains(&format!("the name {name} is {taken}; pick another --name")),
            "--name {name} was not refused as {taken}:\n{stderr}"
        );
    }
    let stderr = refused(&ardana, &[BASE, missing, "--name", "a/b"])?;
    ensure!(stderr.contains("is no name"), "{stderr}");
    Ok(())
}

/// A stand-in GGUF in `dir`: the magic and version alone, which `ardana pull` records without reading further.
fn stand_in(dir: &Path, file: &str) -> Result<String> {
    let gguf = dir.join(file);
    std::fs::write(&gguf, b"GGUF\x03\0\0\0")?;
    Ok(gguf.to_str().context("a UTF-8 path")?.to_string())
}

/// Every path under the hub cache.
fn hub_paths() -> Result<Vec<std::path::PathBuf>> {
    let mut paths = Vec::new();
    let mut dirs = vec![
        std::path::PathBuf::from(std::env::var_os("HF_HOME").context("cargo sets HF_HOME")?)
            .join("hub"),
    ];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() && !path.is_symlink() {
                dirs.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// R3.3: a base whose transformers checkpoint is unknown, an `ollama:` reference (in a store of the test's own), a
/// local GGUF file and decider:4b (the library pins no checkpoint for it), is refused with exit 1 naming
/// `--checkpoint`, before the training environment and any download: the hub cache is as it was. With `--checkpoint`
/// (qwen3:1.7b's checkpoint, as a repository and as its snapshot directory) the first two are read on to their data,
/// whose 99 labelled lines are then refused (decider:4b would read its GGUF repository's tokenizer, which the offline
/// cache lacks).
#[test]
fn unknown_checkpoint_names_the_flag() -> Result<()> {
    let ardana = Ardana::new("finetune-unknown-checkpoint")?;
    let dir = ardana.home.parent().context("the home's directory")?;
    let questions = questions(dir)?;
    let questions = questions.to_str().context("a UTF-8 path")?;
    let line = json!({"state": "ls", "answers": {"risk": "safe", "outside": false}}).to_string();
    let data = data(dir, 98, 0, &line)?;
    let data = data.to_str().context("a UTF-8 path")?;
    let gguf = stand_in(dir, "model.gguf")?;
    let blob = common::ollama_model(&dir.join("ollama"), "qwen3", &"ab".repeat(32), 8)?;
    std::fs::write(&blob, b"GGUF\x03\0\0\0")?;
    let hub = hub_paths()?;
    let flag = "name the checkpoint to train from with --checkpoint hf.co/<org>/<repo> or --checkpoint <dir>";
    for (base, why) in [
        (
            "ollama:qwen3",
            "an Ollama model names no transformers checkpoint",
        ),
        (
            gguf.as_str(),
            "a local GGUF file names no transformers checkpoint",
        ),
        (
            "decider:4b",
            "the library pins no transformers checkpoint for it to train from",
        ),
    ] {
        let stderr = refused(
            &ardana,
            &[base, data, "--questions", questions, "--name", "mine"],
        )?;
        ensure!(
            stderr.contains(&format!("{why}; {flag}")),
            "{base} was not refused naming --checkpoint:\n{stderr}"
        );
    }
    ensure!(hub_paths()? == hub, "a refused run changed the hub cache");

    let snapshot = common::hf_snapshot("Qwen/Qwen3-1.7B")?;
    let commit = snapshot
        .file_name()
        .context("a snapshot")?
        .to_string_lossy()
        .into_owned();
    let snapshot = snapshot.to_str().context("a UTF-8 path")?;
    for (base, checkpoint, read) in [
        (
            "ollama:qwen3",
            "hf.co/Qwen/Qwen3-1.7B",
            format!("hf.co/Qwen/Qwen3-1.7B at {commit}"),
        ),
        (gguf.as_str(), snapshot, snapshot.to_string()),
    ] {
        let stderr = refused(
            &ardana,
            &[
                base,
                data,
                "--questions",
                questions,
                "--name",
                "mine",
                "--checkpoint",
                checkpoint,
            ],
        )?;
        ensure!(
            stderr.contains(&format!("from the checkpoint {read}\n"))
                && stderr.contains(&format!(
                    "{data}: 99 labelled lines; fine-tuning needs at least 100"
                )),
            "{base} with --checkpoint {checkpoint} was not read on to its data:\n{stderr}"
        );
    }
    Ok(())
}

/// R3.4: an entry pulled from `hf.co/Qwen/Qwen3-1.7B-GGUF` resolves, offline, to the checkpoint its repository's card
/// names, `Qwen/Qwen3-1.7B` (its `base_model`), at the commit the hub cache holds: `cargo xtask fetch --tests`
/// installs the card and the checkpoint's `config.json` and tokenizer, no weights. The entry is the one `ardana pull
/// hf.co/Qwen/Qwen3-1.7B-GGUF:Q8_0 --tokenizer hf.co/Qwen/Qwen3-1.7B` records but for its GGUF, a stand-in (the offline
/// cache holds no weights); the run goes on to the data, whose 99 labelled lines it refuses before the environment.
#[test]
fn entry_resolves_checkpoint_from_card() -> Result<()> {
    let ardana = Ardana::new("finetune-entry-card")?;
    let dir = ardana.home.parent().context("the home's directory")?;
    common::hf_file("Qwen/Qwen3-1.7B-GGUF", "README.md")?;
    let gguf = stand_in(dir, "Qwen3-1.7B-Q8_0.gguf")?;
    ardana.ok([
        "pull",
        gguf.as_str(),
        "--tokenizer",
        "hf.co/Qwen/Qwen3-1.7B",
        "--name",
        "qwen3-1.7b",
    ])?;
    let path = ardana.home.join("models.toml");
    let mut file: toml::Table = toml::from_str(&std::fs::read_to_string(&path)?)?;
    let entry = file["model"]
        .as_array_mut()
        .and_then(|models| models.first_mut())
        .and_then(toml::Value::as_table_mut)
        .context("the entry")?;
    entry.insert("source".into(), "hf.co/Qwen/Qwen3-1.7B-GGUF:Q8_0".into());
    std::fs::write(&path, toml::to_string(&file)?)?;
    let shown: Value = serde_json::from_str(&ardana.ok(["show", "qwen3-1.7b", "--json"])?)?;
    ensure!(
        shown["source"] == "hf.co/Qwen/Qwen3-1.7B-GGUF:Q8_0",
        "{shown}"
    );

    let questions = questions(dir)?;
    let questions = questions.to_str().context("a UTF-8 path")?;
    let line = json!({"state": "ls", "answers": {"risk": "safe", "outside": false}}).to_string();
    let data = data(dir, 98, 0, &line)?;
    let data = data.to_str().context("a UTF-8 path")?;
    let stderr = refused(
        &ardana,
        &[
            "qwen3-1.7b",
            data,
            "--questions",
            questions,
            "--name",
            "mine",
        ],
    )?;
    let commit = common::hf_snapshot("Qwen/Qwen3-1.7B")?;
    let commit = commit.file_name().context("a snapshot")?.to_string_lossy();
    ensure!(
        stderr.contains(&format!(
            "fine-tuning qwen3-1.7b from the checkpoint hf.co/Qwen/Qwen3-1.7B at {commit}\n"
        )) && stderr.contains(&format!("{data}: 99 labelled lines")),
        "the entry did not resolve to hf.co/Qwen/Qwen3-1.7B at {commit}:\n{stderr}"
    );
    Ok(())
}

/// R4.2: `--format safetensors` on qwen3.5:0.8b, whose checkpoint's `config.json` (offline, from the hub cache `cargo
/// xtask fetch --tests` fills) names `Qwen3_5ForConditionalGeneration`, no causal LM Candle runs, is refused with exit
/// 1 naming the architecture and `--format gguf`, before the data (its 99 labelled lines would be refused), the
/// training environment and any download: the hub cache is as it was. `--quant` is refused with `--format
/// safetensors`, which writes no GGUF.
#[test]
fn safetensors_refuses_other_architectures() -> Result<()> {
    let ardana = Ardana::new("finetune-safetensors-architectures")?;
    let dir = ardana.home.parent().context("the home's directory")?;
    let questions = questions(dir)?;
    let questions = questions.to_str().context("a UTF-8 path")?;
    let line = json!({"state": "ls", "answers": {"risk": "safe", "outside": false}}).to_string();
    let data = data(dir, 98, 0, &line)?;
    let data = data.to_str().context("a UTF-8 path")?;
    let config = common::hf_file("Qwen/Qwen3.5-0.8B", "config.json")?;
    let config: Value = serde_json::from_str(&std::fs::read_to_string(config)?)?;
    ensure!(
        config["architectures"] == json!(["Qwen3_5ForConditionalGeneration"]),
        "{config}"
    );
    let hub = hub_paths()?;
    let args = [
        "qwen3.5:0.8b",
        data,
        "--questions",
        questions,
        "--name",
        "mine",
        "--format",
        "safetensors",
    ];
    let stderr = refused(&ardana, &args)?;
    ensure!(
        stderr.contains(
            "its architecture Qwen3_5ForConditionalGeneration is no causal LM Candle runs"
        ) && stderr.contains(
            "so --format safetensors cannot write a model ardana runs; pass --format gguf"
        ) && !stderr.contains("labelled lines"),
        "qwen3.5:0.8b was not refused naming its architecture and --format gguf:\n{stderr}"
    );
    let stderr = refused(&ardana, &[&args[..], &["--quant", "q8_0"]].concat())?;
    ensure!(
        stderr.contains("--format safetensors writes no GGUF") && !stderr.contains("fine-tuning "),
        "--quant was not refused with --format safetensors before the base was read:\n{stderr}"
    );
    ensure!(hub_paths()? == hub, "a refused run changed the hub cache");
    Ok(())
}
