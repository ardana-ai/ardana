//! `ardana finetune`'s refusals, before anything heavy happens (R1.4, R1.6, R3.3): data that cannot be trained on is
//! named by file, line and question once the base's tokenizer is read, before the training environment and the
//! checkpoint's weights; a name an entry or the library holds is refused before even the base is read; a base whose
//! checkpoint is unknown is refused naming `--checkpoint`, and an entry pulled from a GGUF repository resolves to the
//! checkpoint its card names (R3.4); `--format safetensors` of a checkpoint Candle does not run is refused (R4.2).
//! `ardana push` of a model `ardana finetune` made lists its files and card and sends nothing under `--dry-run`
//! (R6.1, R6.2), and is refused, sending nothing, for any other entry, a target that is no repository and a missing
//! token (R6.3); its models are stand-ins written as `ardana finetune` writes a model's directory and registered as it
//! registers them. Every run is offline, on the files `cargo xtask fetch` put into `tmp/hf` (qwen3:1.7b's tokenizer
//! and checkpoint listing, which the library snapshot pins, and its GGUF repository's card; qwen3.5:0.8b's tokenizer
//! and `config.json`).

mod common;

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Output;

use anyhow::{Context, Result, bail, ensure};
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

/// A loopback port standing in for the Hub (`HF_ENDPOINT`), which shows that a run sent nothing to it.
struct NoHub(TcpListener);

impl NoHub {
    fn new() -> Result<NoHub> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        Ok(NoHub(listener))
    }

    fn endpoint(&self) -> Result<String> {
        Ok(format!("http://{}", self.0.local_addr()?))
    }

    /// Fails when anything connected: a connection waits in the backlog until it is accepted.
    fn untouched(&self) -> Result<()> {
        match self.0.accept() {
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Ok((_, peer)) => bail!("ardana push connected to the Hub's stand-in from {peer}"),
            Err(err) => Err(err.into()),
        }
    }
}

/// `ardana push <args>` in `ardana`'s home with the Hub at `hub` and a Hugging Face home of its own, `hf/` beside the
/// home, which holds no token; `token` is `HF_TOKEN` (unset for `None`).
fn push(ardana: &Ardana, hub: &NoHub, token: Option<&str>, args: &[&str]) -> Result<Output> {
    let mut cmd = ardana.command(std::iter::once("push").chain(args.iter().copied()));
    cmd.env("HF_ENDPOINT", hub.endpoint()?)
        .env("HF_HOME", ardana.home.with_file_name("hf"))
        .env_remove("HF_TOKEN_PATH");
    match token {
        Some(token) => cmd.env("HF_TOKEN", token),
        None => cmd.env_remove("HF_TOKEN"),
    };
    let output = cmd.output()?;
    println!(
        "ardana push {args:?} ({}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

/// The metrics of one answer type in a report, as `finetune.json` holds them.
fn metrics(answers: u64, accuracy: f64) -> Value {
    json!({"answers": answers, "accuracy": accuracy, "nll": 0.5123, "brier": 0.5, "ece": 0.1234})
}

/// A model as `ardana finetune` leaves it, `models/<name>/` in `ardana`'s home: the base's tokenizer files (Qwen3-1.7B's
/// from the cache), `decider_config.json` in the chat layout, `finetune.json` in the Contract's shape with a report of
/// base against tuned, and stand-in weights, `weights` (file name, bytes), each written as it is; then registered as
/// `ardana finetune` registers it. `record` is merged into the `finetune.json` of a Q8_0 GGUF trained on the CPU from
/// qwen3:1.7b. Returns the directory.
fn finetuned(
    ardana: &Ardana,
    name: &str,
    weights: &[(&str, &[u8])],
    record: Value,
) -> Result<PathBuf> {
    let dir = ardana.home.join("models").join(name);
    std::fs::create_dir_all(&dir)?;
    for file in ["tokenizer.json", "tokenizer_config.json"] {
        std::fs::copy(common::hf_file("Qwen/Qwen3-1.7B", file)?, dir.join(file))?;
    }
    for (file, bytes) in weights {
        std::fs::write(dir.join(file), bytes)?;
    }
    let config = json!({"version": name, "layout": "chat", "isolated_levels": false, "temperature": 1.21,
                        "temperature_by_type": {"choice": 1.3}, "release_date": "2026-10-16"});
    std::fs::write(
        dir.join("decider_config.json"),
        serde_json::to_string_pretty(&config)?,
    )?;
    let commit = common::hf_snapshot("Qwen/Qwen3-1.7B")?;
    let commit = commit.file_name().context("a snapshot")?.to_string_lossy();
    let model = |name: &str, accuracy: f64| {
        json!({"name": name, "metrics": {"choice": metrics(100, accuracy), "noul": metrics(100, accuracy + 0.1),
                                         "overall": metrics(200, accuracy + 0.05)}})
    };
    let mut written = json!({
        "base": "qwen3:1.7b",
        "checkpoint": {"repo": "hf.co/Qwen/Qwen3-1.7B", "commit": commit},
        "format": "gguf",
        "quant": "q8_0",
        "device": "cpu",
        "dtype": "float32",
        "qlora": false,
        "epochs": 2,
        "rows": {"train": 800, "calibration": 100, "test": 100},
        "license": "Apache-2.0",
        "report": {"lines": 100, "base": model("qwen3:1.7b", 0.41), "tuned": model(name, 0.82)},
    });
    for (key, value) in record.as_object().context("a record")? {
        written[key] = value.clone();
    }
    std::fs::write(
        dir.join("finetune.json"),
        serde_json::to_string_pretty(&written)?,
    )?;
    Ok(dir)
}

/// The files `ardana push` lists of `dir`, a model's directory: every file but `finetune.json`, with its bytes.
fn on_disk(dir: &Path) -> Result<Files> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if file != "finetune.json" {
            files.push((file, entry.metadata()?.len()));
        }
    }
    files.sort();
    Ok(files)
}

/// The files of a model's directory, or of a listing, by name with their bytes.
type Files = Vec<(String, u64)>;

/// What a dry run printed: the first line, the files listed with their bytes (`README.md` included) and the card.
fn dry_run(stdout: &str) -> Result<(String, Files, String)> {
    let (listing, card) = stdout
        .split_once("\nREADME.md:\n")
        .context("the dry run printed no card")?;
    let mut lines = listing.lines();
    let first = lines
        .next()
        .context("the dry run printed nothing")?
        .to_string();
    ensure!(
        lines
            .next()
            .map(str::split_whitespace)
            .map(Iterator::collect::<Vec<_>>)
            == Some(vec!["FILE", "BYTES"]),
        "the dry run listed no files:\n{stdout}"
    );
    let mut files = Vec::new();
    for line in lines {
        match line.split_whitespace().collect::<Vec<_>>()[..] {
            [file, bytes] => files.push((file.to_string(), bytes.parse()?)),
            _ => break,
        }
    }
    let card = card
        .strip_suffix('\n')
        .context("the card ends the output")?
        .to_string();
    Ok((first, files, card))
}

/// The front matter of `card`: its lines between the leading `---` and the next.
fn front_matter(card: &str) -> Result<Vec<&str>> {
    let mut lines = card.lines();
    ensure!(
        lines.next() == Some("---"),
        "the card opens no front matter:\n{card}"
    );
    Ok(lines.take_while(|line| *line != "---").collect())
}

/// R6.1, R6.2: `ardana push cmd-risk hf.co/acme/cmd-risk --dry-run` of a GGUF model `ardana finetune` made (registered
/// as it registers one, `ardana pull <its GGUF> --name cmd-risk`, which reads the chat layout its `decider_config.json`
/// names) prints the repository, `private`, every file of the model's directory but `finetune.json` with its bytes on
/// disk and `README.md` with the card's, and the card: front matter with the base's license (as the Hub writes it,
/// lowercase), the checkpoint as
/// `base_model`, `base_model_relation: finetune` and the tags `ardana`, `system-one` and `gguf`; a body with the pull
/// command naming the quant, the run command, the Q23 settings and the test report. Nothing reaches the Hub's
/// stand-in, though `HF_TOKEN` is set. A safetensors model whose checkpoint is a directory of the hub cache and whose
/// base names no license gets the repository alone in its pull command, the `safetensors` tag, the cached repository
/// as `base_model` and no `license`; `--public` says `public`.
#[test]
fn push_dry_run() -> Result<()> {
    let ardana = Ardana::new("push-dry-run")?;
    let hub = NoHub::new()?;
    let gguf: &[u8] = b"GGUF\x03\0\0\0 a stand-in for the quantized model";
    let dir = finetuned(
        &ardana,
        "cmd-risk",
        &[("cmd-risk-Q8_0.gguf", gguf)],
        json!({}),
    )?;
    let weights = dir.join("cmd-risk-Q8_0.gguf");
    ardana.ok([
        "pull",
        weights.to_str().context("a UTF-8 path")?,
        "--name",
        "cmd-risk",
    ])?;
    let entry: Value = serde_json::from_str(&ardana.ok(["show", "cmd-risk", "--json"])?)?;
    ensure!(entry["profile"]["layout"]["kind"] == "chat", "{entry}");

    let output = push(
        &ardana,
        &hub,
        Some("hf_stand_in"),
        &["cmd-risk", "hf.co/acme/cmd-risk", "--dry-run"],
    )?;
    ensure!(
        output.status.success(),
        "the dry run exited {}",
        output.status
    );
    let (first, mut files, card) = dry_run(&String::from_utf8(output.stdout)?)?;
    files.sort();
    ensure!(
        first == "cmd-risk to hf.co/acme/cmd-risk (private); a dry run, nothing is sent",
        "{first}"
    );
    let mut expected = on_disk(&dir)?;
    expected.push(("README.md".into(), card.len() as u64));
    expected.sort();
    ensure!(files == expected, "listed {files:?}, not {expected:?}");
    let names: Vec<&str> = files.iter().map(|(file, _)| file.as_str()).collect();
    ensure!(
        names
            == [
                "README.md",
                "cmd-risk-Q8_0.gguf",
                "decider_config.json",
                "tokenizer.json",
                "tokenizer_config.json"
            ],
        "{names:?}"
    );
    ensure!(
        front_matter(&card)?
            == [
                "license: apache-2.0",
                "base_model: Qwen/Qwen3-1.7B",
                "base_model_relation: finetune",
                "tags:",
                "- ardana",
                "- system-one",
                "- gguf",
            ],
        "{card}"
    );
    for wanted in [
        "```sh\nardana pull hf.co/acme/cmd-risk:Q8_0\nardana run cmd-risk --request request.json\n```",
        "`ardana finetune` made it from qwen3:1.7b, the checkpoint [Qwen/Qwen3-1.7B]",
        "merged and quantized to Q8_0 (`cmd-risk-Q8_0.gguf`)",
        "The license is the base's: Apache-2.0.",
        "A LoRA of rank 16 and alpha 32 on every linear layer, learning rate 1e-4, 2 epochs, seed 0; the epoch with \
         the lowest calibration loss kept.",
        "On cpu in float32.",
        "800 lines to train on, 100 to calibrate on, 100 to test on.",
        "on the 100 test lines",
        "| choice | qwen3:1.7b | 100 | 0.4100 | 0.5123 | 0.5000 | 0.1234 |\n\
         | choice | cmd-risk | 100 | 0.8200 | 0.5123 | 0.5000 | 0.1234 |\n\
         | noul | qwen3:1.7b | 100 | 0.5100 |",
        "| overall | cmd-risk | 200 | 0.8700 |",
    ] {
        ensure!(
            card.contains(wanted),
            "the card holds no `{wanted}`:\n{card}"
        );
    }
    hub.untouched()?;

    // A checkpoint, from a directory in the hub cache, of a base without a license; public.
    let snapshot = common::hf_snapshot("Qwen/Qwen3-1.7B")?;
    let st = finetuned(
        &ardana,
        "cmd-risk-st",
        &[
            ("config.json", br#"{"architectures": ["Qwen3ForCausalLM"]}"#),
            ("model.safetensors", b"a stand-in for the merged checkpoint"),
        ],
        json!({"format": "safetensors", "quant": null, "checkpoint": {"path": snapshot}, "license": null,
               "epochs": 1}),
    )?;
    // Registered as `ardana finetune` registers a checkpoint: the directory as its weights and source, runtime candle.
    let path = ardana.home.join("models.toml");
    let mut file: toml::Table = toml::from_str(&std::fs::read_to_string(&path)?)?;
    let models = file["model"].as_array_mut().context("[[model]]")?;
    let mut entry = models[0].as_table().context("the entry")?.clone();
    let st_dir = st.to_str().context("a UTF-8 path")?;
    for (key, value) in [
        ("name", "cmd-risk-st"),
        ("source", st_dir),
        ("weights", st_dir),
        ("runtime", "candle"),
    ] {
        entry.insert(key.into(), value.into());
    }
    models.push(entry.into());
    std::fs::write(&path, toml::to_string(&file)?)?;
    let output = push(
        &ardana,
        &hub,
        None,
        &[
            "cmd-risk-st",
            "hf.co/acme/cmd-risk-st",
            "--public",
            "--dry-run",
        ],
    )?;
    ensure!(
        output.status.success(),
        "the dry run exited {}",
        output.status
    );
    let (first, mut files, card) = dry_run(&String::from_utf8(output.stdout)?)?;
    files.sort();
    ensure!(
        first == "cmd-risk-st to hf.co/acme/cmd-risk-st (public); a dry run, nothing is sent",
        "{first}"
    );
    let mut expected = on_disk(&st)?;
    expected.push(("README.md".into(), card.len() as u64));
    expected.sort();
    ensure!(files == expected, "listed {files:?}, not {expected:?}");
    ensure!(
        front_matter(&card)?
            == [
                "base_model: Qwen/Qwen3-1.7B",
                "base_model_relation: finetune",
                "tags:",
                "- ardana",
                "- system-one",
                "- safetensors",
            ],
        "{card}"
    );
    for wanted in [
        "```sh\nardana pull hf.co/acme/cmd-risk-st\nardana run cmd-risk-st --request request.json\n```",
        "merged into a safetensors checkpoint in float32, which Candle runs",
        "The base names no license.",
        "1 epoch, seed 0",
    ] {
        ensure!(
            card.contains(wanted),
            "the card holds no `{wanted}`:\n{card}"
        );
    }
    hub.untouched()
}

/// R6.3: `ardana push` exits 1, sending nothing to the Hub's stand-in, for an entry `ardana finetune` did not make
/// (pulled, with no `finetune.json`), a target that is no `hf.co/<org>/<repo>` and, without `--dry-run`, a missing
/// token (`HF_TOKEN` unset or blank and no `$HF_HOME/token`, or a blank one), its message, the last line, naming
/// `HF_TOKEN` and `hf auth login`.
#[test]
fn push_refusals() -> Result<()> {
    let ardana = Ardana::new("push-refusals")?;
    let hub = NoHub::new()?;
    let refused = |token: Option<&str>, args: &[&str], why: &str| -> Result<()> {
        let output = push(&ardana, &hub, token, args)?;
        let stderr = String::from_utf8(output.stderr)?;
        ensure!(
            output.status.code() == Some(1),
            "ardana push {args:?} exited {}",
            output.status
        );
        let last = stderr.lines().last().unwrap_or_default();
        ensure!(
            last.contains(why),
            "ardana push {args:?} was not refused as `{why}`:\n{stderr}"
        );
        ensure!(
            output.stdout.is_empty(),
            "ardana push {args:?} printed what it would push"
        );
        hub.untouched()
    };

    let dir = ardana.home.parent().context("the home's directory")?;
    let pulled = stand_in(dir, "pulled-Q8_0.gguf")?;
    ardana.ok([
        "pull",
        pulled.as_str(),
        "--tokenizer",
        "hf.co/Qwen/Qwen3-1.7B",
        "--name",
        "pulled",
    ])?;
    refused(
        Some("hf_stand_in"),
        &["pulled", "hf.co/acme/pulled"],
        "pulled was not made by `ardana finetune`",
    )?;

    let gguf: &[u8] = b"GGUF\x03\0\0\0";
    let weights = finetuned(
        &ardana,
        "cmd-risk",
        &[("cmd-risk-Q8_0.gguf", gguf)],
        json!({}),
    )?
    .join("cmd-risk-Q8_0.gguf");
    ardana.ok([
        "pull",
        weights.to_str().context("a UTF-8 path")?,
        "--name",
        "cmd-risk",
    ])?;
    for target in [
        "acme/cmd-risk",
        "hf.co/acme",
        "hf.co/acme/cmd-risk:Q8_0",
        "https://huggingface.co/acme/cmd-risk",
    ] {
        refused(
            Some("hf_stand_in"),
            &["cmd-risk", target, "--dry-run"],
            &format!("{target} is no Hugging Face repository; push to hf.co/<org>/<repo>"),
        )?;
    }

    let token = "set HF_TOKEN to a token that can write to acme, or run `hf auth login`";
    refused(None, &["cmd-risk", "hf.co/acme/cmd-risk"], token)?;
    refused(
        Some("  "),
        &["cmd-risk", "hf.co/acme/cmd-risk", "--public"],
        token,
    )?;
    let hf = ardana.home.with_file_name("hf");
    std::fs::create_dir_all(&hf)?;
    std::fs::write(hf.join("token"), "\n")?;
    refused(None, &["cmd-risk", "hf.co/acme/cmd-risk"], token)?;
    Ok(())
}
