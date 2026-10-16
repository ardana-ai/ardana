//! `ardana finetune`'s refusals, before anything heavy happens (R1.4, R1.6): data that cannot be trained on is named
//! by file, line and question once the base's tokenizer is read, before the training environment and the checkpoint's
//! weights; a name an entry or the library holds is refused before even the base is read. Every run is offline, on the
//! tokenizer `cargo xtask fetch` put into `tmp/hf` (qwen3:1.7b's, which the library snapshot pins).

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

/// `ardana finetune <args>` in `ardana`'s home, which must exit 1; returns its stderr.
fn refused(ardana: &Ardana, args: &[&str]) -> Result<String> {
    let output = ardana.output(std::iter::once("finetune").chain(args.iter().copied()))?;
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
