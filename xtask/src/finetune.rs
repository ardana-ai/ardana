//! `cargo xtask e2e finetune`: each row of [`crate::e2e::FINETUNE_FIXTURES`] in turn, with the release `ardana`, online,
//! on one suite home, `tmp/e2e/finetune/home`, over the sandbox's hub cache. The suite first writes the data the rows
//! name into its directory: the first 1,000 lines of decider@23579f7's `teacher_data/commands.jsonl` (Q22) as lines of
//! `ardana finetune`'s format and as CSV, their two shared questions in a questions file, and the first 200 lines of
//! its `teacher_data/custom_questions.jsonl`, each with its own questions; then it starts one `ardana serve` on the
//! home, which runs until the suite ends. A row that answers fine-tunes its base three times: stopped by SIGINT
//! mid-training, then with a trainer that exits non-zero (its script moved out of the venv and back), neither leaving
//! an entry or a model directory, then to the end; its model (a GGUF, or under `--format safetensors` a checkpoint
//! Candle runs and was calibrated through) and its report of base against tuned on the test split are checked, its
//! model answers through `ardana run` and through the running server alike, `ardana push --dry-run` lists its files as
//! they are on disk, and `ardana rm` removes it. A row passing
//! `--trust-remote-code` is first refused without it, before any weight downloads; a row whose base is an `hf.co/` GGUF
//! reference then fine-tunes again from that GGUF's file in the hub cache with `--checkpoint <the checkpoint's
//! snapshot directory>` and `--epochs 1`. The rows after the first that answers find the training environment as that
//! row left it. A refused row's run exits 1 naming why, before any weight downloads; one passing `--device` names the
//! memory estimate above the machine's, and the same run with `--device cuda` is refused naming the devices torch
//! finds. Whatever a row's checks found, the entries, model directories and hub paths it added are removed when they
//! end; the training environment in the home stays for the next row, and the suite removes the home when it ends, also
//! when a row failed.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use ardana_registry::base::Card;
use ardana_registry::library::{Library, LibraryPick};
use ardana_registry::refs::{HfFile, Ref, matches_quant};

use crate::e2e::{Expect, FinetuneFixture, Gain, read_json};
use crate::hub;
use crate::sandbox::{Sandbox, run_stdout};
use crate::serve::{Server, build_ardana};

/// The commands data, in the suite's directory: its lines, the same lines as CSV, and their shared questions.
pub const COMMANDS: &str = "commands.jsonl";
pub const COMMANDS_CSV: &str = "commands.csv";
pub const COMMANDS_QUESTIONS: &str = "commands.questions.json";
/// The custom questions data, in the suite's directory: lines that carry their own questions.
pub const CUSTOM: &str = "custom.jsonl";
/// The teacher data of the decider clone `cargo xtask fetch` makes (decider@23579f7, Apache-2.0).
const TEACHER_COMMANDS: &str = "src/decider/teacher_data/commands.jsonl";
const TEACHER_CUSTOM: &str = "src/decider/teacher_data/custom_questions.jsonl";
/// The lines of it the commands data takes, and the ids its two questions get (the risk choice, then the noul).
const COMMANDS_LINES: usize = 1000;
const QUESTION_IDS: [&str; 2] = ["risk", "outside"];
/// The lines the custom questions data takes (Q22); a line's questions get the ids `q1`, `q2`, ... in its order.
const CUSTOM_LINES: usize = 200;
/// What a run prints when it downloads or builds a part of the training environment: `ardana`'s own lines and uv's.
const ENVIRONMENT_BUILT: [&str; 5] = [
    "downloading uv",
    "downloading llama.cpp",
    "creating the training environment",
    "Downloading ",
    "Installed ",
];
/// The Q29 pins the venv must hold, as `importlib.metadata` reports them (torch from PyTorch's CPU wheels), and the
/// interpreter (Q3) and converter (Q25) the environment is built with.
const PINS: [(&str, &str); 11] = [
    ("torch", "2.14.1+cpu"),
    ("transformers", "5.18.0"),
    ("peft", "0.21.2"),
    ("trl", "1.16.0"),
    ("accelerate", "1.15.0"),
    ("datasets", "5.1.0"),
    ("safetensors", "0.8.0"),
    ("tokenizers", "0.23.2"),
    ("sentencepiece", "0.2.2"),
    ("protobuf", "7.36.2"),
    ("numpy", "2.5.3"),
];
const UV: &str = "uv 0.13.1";
const PYTHON: &str = "3.12";
const CONVERTER: &str = "llama.cpp-26394b4e6749a41c3633db040e0987500a5f7013";
/// The trainer's script in the venv, which the failing run moves aside.
const TRAINER: &str = "venv/lib/python3.12/site-packages/trl/scripts/sft.py";
/// The GGUF `general.file_type` of each quant a row checks, as llama.cpp numbers them.
const FILE_TYPES: [(&str, u64); 3] = [("q4_0", 2), ("q8_0", 7), ("q4_k_m", 15)];
/// How long the interrupted run may take to reach its first training step (the environment and the checkpoint
/// download first), and to exit once interrupted.
const FIRST_STEP: Duration = Duration::from_secs(3600);
const STOPPING: Duration = Duration::from_secs(120);

/// Runs every row, a failed one counted but not ending the suite, then stops the server and removes the suite's
/// directory.
pub fn finetune(sandbox: &Sandbox, fixtures: &[FinetuneFixture]) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let dir = sandbox.tmp().join("e2e/finetune");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("emptying {}", dir.display()))?;
    }
    let home = dir.join("home");
    std::fs::create_dir_all(&home)?;
    let cli = Cli {
        sandbox,
        ardana: &ardana,
        dir: dir.clone(),
        home,
    };
    let mut failed = Vec::new();
    let started = commands_data(sandbox, &dir)
        .and_then(|()| custom_data(sandbox, &dir))
        .and_then(|()| Server::start_on(sandbox, &ardana, &cli.home));
    match started {
        Ok(server) => {
            // The training environment as the first row that answers left it.
            let mut environment = None;
            for fixture in fixtures {
                let started = Instant::now();
                match row(&cli, &server, fixture, &mut environment) {
                    Ok(()) => println!(
                        "e2e finetune {}: passed in {:.1} min",
                        fixture.name,
                        started.elapsed().as_secs_f64() / 60.0
                    ),
                    Err(err) => {
                        println!("e2e finetune {}: failed: {err:#}", fixture.name);
                        failed.push(fixture.name);
                    }
                }
            }
            if let Err(err) = server.stop() {
                println!("e2e finetune: stopping the server: {err:#}");
                failed.push("serve");
            }
        }
        Err(err) => {
            println!("e2e finetune: writing the data or starting the server failed: {err:#}");
            failed.push("data");
        }
    }
    let removed =
        std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()));
    if removed.is_ok() {
        println!(
            "e2e finetune: removed the suite's home {}",
            cli.home.display()
        );
    }
    ensure!(
        failed.is_empty(),
        "{} of {} finetune rows failed: {}",
        failed.len(),
        fixtures.len(),
        failed.join(", ")
    );
    removed
}

/// Q22's commands data in `dir`: each of the first [`COMMANDS_LINES`] lines of the teacher data as `{"state",
/// "answers"}`, and as a record of [`COMMANDS_CSV`] (a `state` column and one per question id, a noul written `true`
/// or `false`); its two questions, which every line shares, in [`COMMANDS_QUESTIONS`] under [`QUESTION_IDS`].
fn commands_data(sandbox: &Sandbox, dir: &Path) -> Result<()> {
    let source = sandbox.tmp().join(TEACHER_COMMANDS);
    let text = std::fs::read_to_string(&source)
        .with_context(|| format!("reading {}; run `cargo xtask fetch`", source.display()))?;
    let mut shared: Option<Value> = None;
    let mut lines = String::new();
    let mut csv = format!("state,{}\n", QUESTION_IDS.join(","));
    for (i, line) in text.lines().take(COMMANDS_LINES).enumerate() {
        let teacher: Value = serde_json::from_str(line)
            .with_context(|| format!("{}:{}", source.display(), i + 1))?;
        let asked = teacher["questions"]
            .as_array()
            .filter(|questions| questions.len() == QUESTION_IDS.len())
            .with_context(|| format!("{}:{} asks no two questions", source.display(), i + 1))?;
        let mut questions = serde_json::Map::new();
        let mut answers = serde_json::Map::new();
        for (id, question) in QUESTION_IDS.iter().zip(asked) {
            let mut spec = serde_json::Map::new();
            for key in ["type", "instructions", "criteria"] {
                if let Some(value) = question.get(key) {
                    spec.insert(key.into(), value.clone());
                }
            }
            questions.insert(id.to_string(), spec.into());
            answers.insert(id.to_string(), question["answer"].clone());
        }
        let questions = Value::Object(questions);
        let shared = shared.get_or_insert_with(|| questions.clone());
        ensure!(
            *shared == questions,
            "{}:{} asks other questions than the first line",
            source.display(),
            i + 1
        );
        let state = teacher["state"]
            .as_str()
            .with_context(|| format!("{}:{}'s state is no text", source.display(), i + 1))?;
        let mut cells = vec![csv_cell(state)];
        for id in QUESTION_IDS {
            cells.push(match &answers[id] {
                Value::String(label) => csv_cell(label),
                Value::Bool(yes) => yes.to_string(),
                other => bail!("{}:{} answers {id} with {other}", source.display(), i + 1),
            });
        }
        csv.push_str(&cells.join(","));
        csv.push('\n');
        lines.push_str(&json!({"state": teacher["state"], "answers": answers}).to_string());
        lines.push('\n');
    }
    std::fs::write(dir.join(COMMANDS), lines)?;
    std::fs::write(dir.join(COMMANDS_CSV), csv)?;
    let shared = shared.context("the teacher data is empty")?;
    std::fs::write(
        dir.join(COMMANDS_QUESTIONS),
        serde_json::to_string_pretty(&shared)?,
    )?;
    println!(
        "e2e finetune: wrote {COMMANDS} and {COMMANDS_CSV} ({COMMANDS_LINES} lines of {}) and {COMMANDS_QUESTIONS}",
        source.display()
    );
    Ok(())
}

/// A CSV cell: quoted when it holds a comma, a quote or a line break, its quotes doubled.
fn csv_cell(text: &str) -> String {
    if text.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// Q22's custom questions data in `dir`: each of the first [`CUSTOM_LINES`] lines of the teacher data as `{"state",
/// "questions", "answers"}`, its questions under the ids `q1`, `q2`, ... and each answered with its teacher label.
fn custom_data(sandbox: &Sandbox, dir: &Path) -> Result<()> {
    let source = sandbox.tmp().join(TEACHER_CUSTOM);
    let text = std::fs::read_to_string(&source)
        .with_context(|| format!("reading {}; run `cargo xtask fetch`", source.display()))?;
    let mut lines = String::new();
    for (i, line) in text.lines().take(CUSTOM_LINES).enumerate() {
        let teacher: Value = serde_json::from_str(line)
            .with_context(|| format!("{}:{}", source.display(), i + 1))?;
        let asked = teacher["questions"]
            .as_array()
            .with_context(|| format!("{}:{} asks no questions", source.display(), i + 1))?;
        let mut questions = serde_json::Map::new();
        let mut answers = serde_json::Map::new();
        for (j, question) in asked.iter().enumerate() {
            let id = format!("q{}", j + 1);
            let mut spec = serde_json::Map::new();
            for key in ["type", "instructions", "criteria"] {
                if let Some(value) = question.get(key) {
                    spec.insert(key.into(), value.clone());
                }
            }
            questions.insert(id.clone(), spec.into());
            answers.insert(id, question["answer"].clone());
        }
        lines.push_str(
            &json!({"state": teacher["state"], "questions": questions, "answers": answers})
                .to_string(),
        );
        lines.push('\n');
    }
    std::fs::write(dir.join(CUSTOM), lines)?;
    println!(
        "e2e finetune: wrote {CUSTOM} ({CUSTOM_LINES} lines of {})",
        source.display()
    );
    Ok(())
}

/// One row: its checks, then, whatever they found, every entry it added (the models it fine-tunes, the base it pulled),
/// the models' directories and every path it added to the hub cache removed. A row that answers after the first finds
/// the training environment as it was (R2.5); the first records it in `environment`.
fn row(
    cli: &Cli,
    server: &Server,
    fixture: &FinetuneFixture,
    environment: &mut Option<Files>,
) -> Result<()> {
    let hub = hub::dir(cli.sandbox);
    let before = hub::paths(&hub)?;
    let entries = cli.entries()?;
    let run = Run::of(fixture)?;
    let local = format!("{}-local", run.name);
    let checked = match fixture.expect {
        Expect::Answers => untrusted(cli, &run, &before)
            .and_then(|()| answers(cli, server, &run))
            .and_then(|record| from_local_gguf(cli, server, &run, &local, &record))
            .and_then(|()| {
                let files = cli.environment_files()?;
                match environment {
                    Some(first) => cli.reused(&run, first, &files),
                    None => {
                        println!(
                            "e2e finetune {}: the training environment holds {} files",
                            run.row,
                            files.len()
                        );
                        *environment = Some(files);
                        Ok(())
                    }
                }
            }),
        Expect::Refused(why) => refused(cli, &run, why, &before, "")
            .and_then(|message| limits(cli, &run, &message, &before)),
    };
    let removed = cli
        .remove(&[run.name.as_str(), local.as_str()], &entries)
        .and(hub::remove_added(&hub, &before))
        .context("removing what the row added");
    match &removed {
        Ok(bytes) => println!(
            "e2e finetune {}: removed {} and {:.2} GB from {}",
            run.row,
            run.name,
            *bytes as f64 / 1e9,
            hub.display()
        ),
        Err(err) => println!("e2e finetune {}: {err:#}", run.row),
    }
    checked.and(removed.map(drop))
}

/// R3.5 for a row that passes `--trust-remote-code`: the same run without it exits 1 naming the flag, before any
/// weight downloads (its checkpoint's `config.json` has `auto_map`). Nothing for any other row.
fn untrusted(cli: &Cli, run: &Run, before: &BTreeSet<PathBuf>) -> Result<()> {
    const FLAG: &str = "--trust-remote-code";
    if !run.args.iter().any(|arg| arg == FLAG) {
        return Ok(());
    }
    let without = Run {
        args: run
            .args
            .iter()
            .filter(|arg| *arg != FLAG)
            .cloned()
            .collect(),
        ..run.clone()
    };
    refused(cli, &without, FLAG, before, "-untrusted").map(drop)
}

/// R5.2 and R5.3 for a refused row that passes `--device`: its refusal `message` names the memory the run needs and
/// the memory it has, the first the larger, and the same run with `--device cuda` exits 1 naming the devices torch
/// finds, before any weight downloads (the suite's machine has no CUDA). Nothing for any other row.
fn limits(cli: &Cli, run: &Run, message: &str, before: &BTreeSet<PathBuf>) -> Result<()> {
    if run.flag("--device").is_none() {
        return Ok(());
    }
    let sizes = sizes(message);
    ensure!(
        matches!(sizes[..], [needs, has] if needs > has),
        "the refusal names the sizes {sizes:?}, not the memory the run needs and the smaller memory it has: {message}"
    );
    let mut args = run.args.clone();
    let at = args
        .iter()
        .position(|arg| arg == "--device")
        .context("the row passes no --device")?;
    args[at + 1] = "cuda".into();
    let cuda = Run {
        args,
        ..run.clone()
    };
    let message = refused(
        cli,
        &cuda,
        "--device cuda: torch finds no CUDA device",
        before,
        "-cuda",
    )?;
    ensure!(
        message.ends_with("it finds cpu") || message.ends_with("it finds mps, cpu"),
        "the refusal of --device cuda names no devices torch finds: {message}"
    );
    Ok(())
}

/// The sizes `text` names as `human_size` writes them (`62.6 GB`), in bytes, in order.
fn sizes(text: &str) -> Vec<f64> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .windows(2)
        .filter_map(|pair| {
            let value: f64 = pair[0].parse().ok()?;
            let unit = pair[1].trim_end_matches(|c: char| !c.is_ascii_alphabetic());
            let scale = match unit {
                "KB" => 1e3,
                "MB" => 1e6,
                "GB" => 1e9,
                "TB" => 1e12,
                _ => return None,
            };
            Some(value * scale)
        })
        .collect()
}

/// R1.1, R1.2, R1.7 and R2.2 to R2.4 for a row that answers: a run stopped by SIGINT mid-training and a run whose
/// trainer exits non-zero leave no entry and no model directory, the first having installed the environment; then
/// the fine-tune runs to the end and is checked ([`finished`]). Returns the model's `finetune.json`.
fn answers(cli: &Cli, server: &Server, run: &Run) -> Result<Value> {
    let (row, name) = (run.row, run.name.as_str());
    let log = cli.dir.join(format!("{name}-interrupted.log"));
    let started = Instant::now();
    let mut child = cli.spawn(run, &log, None)?;
    let stopped = interrupt_mid_training(cli, &mut child, &log);
    let status = match stopped {
        Ok(status) => status,
        Err(err) => {
            child.kill().ok();
            child.wait().ok();
            return Err(err.context(format!("the interrupted run; its log: {}", log.display())));
        }
    };
    let text = std::fs::read_to_string(&log)?;
    ensure!(
        !status.success() && text.contains("interrupted"),
        "the interrupted run exited {status}:\n{}",
        tail(&text)
    );
    cli.left_nothing(name)?;
    println!(
        "e2e finetune {row}: SIGINT mid-training ended the run ({status}) after {:.1} min, leaving no entry and no \
         model directory",
        started.elapsed().as_secs_f64() / 60.0
    );
    cli.environment()?;
    println!(
        "e2e finetune {row}: the environment holds {UV}, CPython {PYTHON}, the Q29 pins and {CONVERTER}'s converter"
    );

    // The trainer exits 1, its script gone from the venv; the script is put back whatever the run did.
    let trainer = cli.home.join("finetune").join(TRAINER);
    let aside = trainer.with_extension("py.aside");
    std::fs::rename(&trainer, &aside)
        .with_context(|| format!("moving {} aside", trainer.display()))?;
    let log = cli.dir.join(format!("{name}-failing.log"));
    let failing = cli
        .spawn(run, &log, None)
        .and_then(|mut child| Ok(child.wait()?));
    std::fs::rename(&aside, &trainer)
        .with_context(|| format!("putting {} back", trainer.display()))?;
    let status = failing?;
    let text = std::fs::read_to_string(&log)?;
    ensure!(
        status.code() == Some(1) && text.contains("the trainer (TRL sft) failed (exit status: 1)"),
        "the run with a failing trainer exited {status}:\n{}",
        tail(&text)
    );
    cli.left_nothing(name)?;
    println!(
        "e2e finetune {row}: a trainer exiting 1 ended the run, leaving no entry and no model directory"
    );
    finished(cli, server, run)
}

/// A fine-tune run to the end: its directory holds the GGUF in the expected quant, the tokenizer and chat template
/// files of the base, `decider_config.json` in the base's layout with the fitted temperatures and `finetune.json` with
/// the report its stdout prints, `ardana run` answers every question with it and the running server answers alike,
/// `ardana push --dry-run` lists its files as they are on disk, and `ardana rm` removes the entry and the directory.
/// Returns the model's `finetune.json`.
fn finished(cli: &Cli, server: &Server, run: &Run) -> Result<Value> {
    let (row, name) = (run.row, run.name.as_str());
    let log = cli.dir.join(format!("{name}.log"));
    let out = cli.dir.join(format!("{name}.out"));
    let started = Instant::now();
    let status = cli.spawn(run, &log, Some(&out))?.wait()?;
    let text = std::fs::read_to_string(&log)?;
    let printed = std::fs::read_to_string(&out)?;
    ensure!(
        status.success(),
        "the fine-tune exited {status}:\n{}{printed}",
        tail(&text)
    );
    println!(
        "e2e finetune {row}: fine-tuned {name} in {:.1} min",
        started.elapsed().as_secs_f64() / 60.0
    );
    // The calibration loss of every epoch, of which the trainer kept the lowest.
    let evaluated: Vec<&str> = text
        .split(['\r', '\n'])
        .filter_map(|line| line.find("{'eval_loss'").map(|at| &line[at..]))
        .collect();
    let epochs = run.epochs()?;
    ensure!(
        evaluated.len() as u64 == epochs,
        "the trainer evaluated the calibration split {} times in {epochs} epochs",
        evaluated.len()
    );
    for metrics in evaluated {
        println!("e2e finetune {row}: {metrics}");
    }
    let expected = cli.expected(run)?;
    let record = cli.model(run, &expected)?;
    cli.calibrated(run, &text)?;
    cli.reported(run, &expected, &printed)?;
    let request = cli.request(run)?;
    let response = cli.answered(name, &request)?;
    cli.served(server, name, &request, &response)?;
    cli.pushed(run)?;
    let removed = run_stdout(cli.command().args(["rm", name]))?;
    ensure!(
        !cli.home.join("models").join(name).exists() && !cli.listed(name)?,
        "ardana rm {name} left its entry or its directory: {removed}"
    );
    println!("e2e finetune {row}: {}", removed.trim());
    Ok(record)
}

/// R3.2 for a row whose base is an `hf.co/` GGUF reference: the same data fine-tuned again, as `local`, from that GGUF's
/// file in the hub cache, with `--checkpoint <the snapshot directory of the checkpoint the first run recorded>` and
/// `--epochs 1`, run to the end ([`finished`]), its `finetune.json` recording the directory as `{"path"}`. Nothing for
/// any other row.
fn from_local_gguf(
    cli: &Cli,
    server: &Server,
    run: &Run,
    local: &str,
    record: &Value,
) -> Result<()> {
    let Ok(Ref::Hf {
        org,
        repo,
        file: Some(file),
    }) = Ref::parse(&run.base)
    else {
        return Ok(());
    };
    let hub = hub::dir(cli.sandbox);
    let gguf = cached_gguf(&hub, &org, &repo, &file)?;
    let (checkpoint, commit) = (
        record["checkpoint"]["repo"].as_str(),
        record["checkpoint"]["commit"].as_str(),
    );
    let (Some(checkpoint), Some(commit)) = (checkpoint, commit) else {
        bail!(
            "{}'s finetune.json records the checkpoint {}, no repository at a commit",
            run.name,
            record["checkpoint"]
        );
    };
    let dir = hub
        .join(repo_dir(checkpoint.trim_start_matches("hf.co/")))
        .join("snapshots")
        .join(commit);
    let mut args: Vec<String> = Vec::new();
    let mut given = run.args.iter();
    while let Some(arg) = given.next() {
        match arg.as_str() {
            "--name" | "--epochs" | "--checkpoint" => {
                given.next();
            }
            _ => args.push(arg.clone()),
        }
    }
    args.extend(["--name".into(), local.into(), "--epochs".into(), "1".into()]);
    args.extend(["--checkpoint".into(), dir.display().to_string()]);
    println!(
        "e2e finetune {}: fine-tuning {local} from the GGUF {} with --checkpoint {}",
        run.row,
        gguf.display(),
        dir.display()
    );
    let again = Run {
        row: run.row,
        base: gguf.display().to_string(),
        args,
        name: local.to_string(),
        gain: run.gain,
    };
    finished(cli, server, &again).map(drop)
}

/// The one GGUF of `org/repo` the reference's `file` names (its quant, or its path), in a snapshot of the hub cache.
fn cached_gguf(hub: &Path, org: &str, repo: &str, file: &HfFile) -> Result<PathBuf> {
    let snapshots = hub
        .join(repo_dir(&format!("{org}/{repo}")))
        .join("snapshots");
    let mut found = Vec::new();
    for snapshot in
        std::fs::read_dir(&snapshots).with_context(|| format!("reading {}", snapshots.display()))?
    {
        let snapshot = snapshot?.path();
        for entry in std::fs::read_dir(&snapshot)? {
            let path = entry?.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let picked = match file {
                HfFile::Quant(quant) => matches_quant(&name, quant),
                HfFile::Name(wanted) => snapshot.join(wanted) == path,
            };
            if picked {
                found.push(path);
            }
        }
    }
    match &found[..] {
        [one] => Ok(one.clone()),
        _ => bail!(
            "{} holds {} GGUFs the reference names, not one: {found:?}",
            snapshots.display(),
            found.len()
        ),
    }
}

/// The checkpoint `repo` (`<org>/<repo>`) as `finetune.json` records it, `{"repo", "commit"}` with the commit of the one
/// snapshot in the hub cache holding `config.json`, and that snapshot's directory.
fn cached_checkpoint(hub: &Path, repo: &str) -> Result<(Value, PathBuf)> {
    let snapshots = hub.join(repo_dir(repo)).join("snapshots");
    let held: Vec<PathBuf> = std::fs::read_dir(&snapshots)
        .with_context(|| format!("reading {}", snapshots.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|dir| dir.join("config.json").is_file())
        .collect();
    let [dir] = &held[..] else {
        bail!(
            "{} holds {} checkpoints, not one",
            snapshots.display(),
            held.len()
        );
    };
    let commit = dir.file_name().unwrap_or_default().to_string_lossy();
    Ok((
        json!({"repo": format!("hf.co/{repo}"), "commit": commit}),
        dir.clone(),
    ))
}

/// `models--<org>--<repo>`, the hub cache's directory of `org/repo`.
fn repo_dir(repo: &str) -> String {
    format!("models--{}", repo.replace('/', "--"))
}

/// One fine-tune of a row: its base, its arguments (the data first) and the model's name.
#[derive(Debug, Clone)]
struct Run {
    row: &'static str,
    base: String,
    args: Vec<String>,
    name: String,
    gain: Gain,
}

impl Run {
    /// The fine-tune a row names.
    fn of(fixture: &FinetuneFixture) -> Result<Run> {
        let args: Vec<String> = fixture.args.iter().map(|arg| arg.to_string()).collect();
        let name = flag(&args, "--name")
            .context("the row names no --name")?
            .to_string();
        Ok(Run {
            row: fixture.name,
            base: fixture.base.to_string(),
            args,
            name,
            gain: fixture.gain,
        })
    }

    /// The value the run's args give `flag`.
    fn flag(&self, name: &str) -> Option<&str> {
        flag(&self.args, name)
    }

    /// What the run writes: its `--format`, else `ardana finetune`'s default, `gguf` (Q9).
    fn format(&self) -> &str {
        self.flag("--format").unwrap_or("gguf")
    }

    /// The epochs the run trains: its `--epochs`, else `ardana finetune`'s default (Q23).
    fn epochs(&self) -> Result<u64> {
        Ok(self.flag("--epochs").map_or(Ok(2), str::parse)?)
    }

    /// The data file the run names, relative to the suite's directory.
    fn data(&self) -> Result<&str> {
        self.args
            .first()
            .map(String::as_str)
            .context("the row names no data")
    }
}

/// The value `args` give `flag`.
fn flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

/// Waits for the run's first training step in its log, sends the process SIGINT, and waits for it to exit.
fn interrupt_mid_training(
    cli: &Cli,
    child: &mut Child,
    log: &Path,
) -> Result<std::process::ExitStatus> {
    let waiting = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            // The suite's directory, the log with it, goes when the suite ends: the failure carries its end.
            let text = std::fs::read_to_string(log).unwrap_or_default();
            bail!(
                "the run exited ({status}) before its first training step:\n{}",
                tail(&text)
            );
        }
        if std::fs::read_to_string(log).is_ok_and(|text| training(&text)) {
            break;
        }
        ensure!(
            waiting.elapsed() < FIRST_STEP,
            "no training step within {} min",
            FIRST_STEP.as_secs() / 60
        );
        std::thread::sleep(Duration::from_secs(2));
    }
    run_stdout(
        cli.sandbox
            .command("kill")
            .args(["-INT", &child.id().to_string()]),
    )?;
    let stopping = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        ensure!(
            stopping.elapsed() < STOPPING,
            "the run did not exit within {} s of SIGINT",
            STOPPING.as_secs()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Whether `log`, a run's output, shows a training step done: after ardana's `training` line, a progress bar of the
/// trainer's (no label of its own, unlike the bars that load weights and data) past its first step.
fn training(log: &str) -> bool {
    let Some((_, after)) = log.split_once("\ntraining ") else {
        return false;
    };
    after.split(['\r', '\n']).any(|bar| {
        let bar = bar.trim_start();
        bar.starts_with(|c: char| c.is_ascii_digit())
            && bar.contains("%|")
            && bar.rsplit_once("| ").is_some_and(|(_, rest)| {
                rest.split_once('/')
                    .and_then(|(done, _)| done.trim().parse::<u64>().ok())
                    .is_some_and(|done| done >= 1)
            })
    })
}

/// A refused run: `ardana finetune` exits 1 with `why` in its message, before any weight download (no file the hub
/// cache gained since `before` is a weight file), and leaves no entry and no model directory; its log is
/// `<name><suffix>.log`. Returns the line of its message holding `why`.
fn refused(
    cli: &Cli,
    run: &Run,
    why: &str,
    before: &BTreeSet<PathBuf>,
    suffix: &str,
) -> Result<String> {
    let name = run.name.as_str();
    let log = cli.dir.join(format!("{name}{suffix}.log"));
    let status = cli.spawn(run, &log, None)?.wait()?;
    let text = std::fs::read_to_string(&log)?;
    ensure!(
        status.code() == Some(1) && text.contains(why),
        "ardana finetune {} exited {status}, not refusing it as `{why}`:\n{}",
        run.base,
        tail(&text)
    );
    let added: Vec<PathBuf> = hub::paths(&hub::dir(cli.sandbox))?
        .into_iter()
        .filter(|path| !before.contains(path) && is_weights(path))
        .collect();
    ensure!(added.is_empty(), "the refused run downloaded {added:?}");
    cli.left_nothing(name)?;
    let message = text
        .lines()
        .find(|line| line.contains(why))
        .unwrap_or_default();
    println!("e2e finetune {}: refused, {message}", run.row);
    Ok(message.to_string())
}

fn is_weights(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    [".safetensors", ".gguf", ".bin", ".pth", ".onnx"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// The last lines of a run's output, for a failure.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(30)..].join("\n")
}

/// The files of the training environment, by path relative to it: their size and modification time.
type Files = BTreeMap<PathBuf, (u64, SystemTime)>;

/// The release `ardana` on the suite home, run from the suite's directory, where the rows' data files are.
struct Cli<'a> {
    sandbox: &'a Sandbox,
    ardana: &'a Path,
    dir: PathBuf,
    home: PathBuf,
}

impl Cli<'_> {
    fn command(&self) -> Command {
        let mut cmd = self.sandbox.command(self.ardana);
        cmd.current_dir(&self.dir)
            .env("ARDANA_HOME", &self.home)
            .env("ARDANA_NO_UPDATE_CHECK", "1");
        cmd
    }

    /// `ardana finetune <base> <args>`, its stderr going to `log` and its stdout to `out`, else to `log` too.
    fn spawn(&self, run: &Run, log: &Path, out: Option<&Path>) -> Result<Child> {
        let create = |path: &Path| {
            std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))
        };
        let file = create(log)?;
        let stdout = match out {
            Some(out) => create(out)?,
            None => file.try_clone()?,
        };
        let mut cmd = self.command();
        cmd.arg("finetune")
            .arg(&run.base)
            .args(&run.args)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(file);
        cmd.spawn().context("starting ardana finetune")
    }

    /// The names `ardana list` lists.
    fn entries(&self) -> Result<Vec<String>> {
        let listed = run_stdout(self.command().arg("list"))?;
        Ok(listed
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_string)
            .collect())
    }

    /// Whether `ardana list` lists `name`.
    fn listed(&self, name: &str) -> Result<bool> {
        Ok(self.entries()?.iter().any(|entry| entry == name))
    }

    /// R1.7: no entry named `name`, no model directory and no working directory of a run that builds it
    /// (`finetune/work/<name>-<pid>`).
    fn left_nothing(&self, name: &str) -> Result<()> {
        ensure!(!self.listed(name)?, "the run left the entry {name}");
        let models = self.home.join("models").join(name);
        ensure!(!models.exists(), "the run left {}", models.display());
        let work = self.home.join("finetune/work");
        if work.is_dir() {
            for entry in std::fs::read_dir(&work)? {
                let dir = entry?.file_name();
                let pid = dir
                    .to_str()
                    .and_then(|dir| dir.strip_prefix(name)?.strip_prefix('-'));
                ensure!(
                    !pid.is_some_and(
                        |pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit())
                    ),
                    "the run left {}",
                    work.join(&dir).display()
                );
            }
        }
        Ok(())
    }

    /// R1.2: `$ARDANA_HOME/finetune/` holds uv 0.13.1, a venv on uv's CPython 3.12 from the same directory with the
    /// Q29 pins, and the converter with its `conversion/` and `gguf-py/`.
    fn environment(&self) -> Result<()> {
        let root = self.home.join("finetune");
        let uv = run_stdout(self.sandbox.command(root.join("bin/uv")).arg("--version"))?;
        ensure!(
            uv.starts_with(&format!("{UV} ")),
            "the environment's uv is {uv}"
        );
        let python = root.join("venv/bin/python");
        let check = format!(
            "import importlib.metadata as m, sys; print(sys.version.split()[0], sys.base_prefix); [print(p, m.version(p)) for p in {:?}]",
            PINS.iter().map(|(name, _)| *name).collect::<Vec<_>>()
        );
        let report = run_stdout(
            self.sandbox
                .command(&python)
                .args(["-B", "-I", "-c", &check]),
        )?;
        let mut lines = report.lines();
        let (version, prefix) = lines
            .next()
            .and_then(|line| line.split_once(' '))
            .context("the venv's interpreter")?;
        ensure!(
            version.starts_with(&format!("{PYTHON}."))
                && Path::new(prefix).starts_with(root.join("python")),
            "the venv runs Python {version} from {prefix}"
        );
        for ((package, pinned), line) in PINS.iter().zip(lines) {
            ensure!(
                line == format!("{package} {pinned}"),
                "the venv holds {line}, not {package} {pinned}"
            );
        }
        let converter = root.join(CONVERTER);
        for part in [
            "convert_hf_to_gguf.py",
            "conversion/__init__.py",
            "gguf-py/gguf/__init__.py",
        ] {
            ensure!(
                converter.join(part).is_file(),
                "the converter has no {part} in {}",
                converter.display()
            );
        }
        Ok(())
    }

    /// R6.4: `ardana push <name> hf.co/ardana-e2e/<name> --dry-run` lists the model's real files: every file of its
    /// directory but `finetune.json`, with its bytes on disk, and `README.md` with the bytes of the card it prints, the
    /// repository private.
    fn pushed(&self, run: &Run) -> Result<()> {
        let name = run.name.as_str();
        let target = format!("hf.co/ardana-e2e/{name}");
        let printed =
            run_stdout(
                self.command()
                    .args(["push", name, target.as_str(), "--dry-run"]),
            )?;
        let (listing, card) = printed
            .split_once("\nREADME.md:\n")
            .with_context(|| format!("ardana push --dry-run printed no card:\n{printed}"))?;
        let card = card.strip_suffix('\n').unwrap_or(card);
        let mut lines = listing.lines();
        let first = lines.next().unwrap_or_default();
        ensure!(
            first == format!("{name} to {target} (private); a dry run, nothing is sent"),
            "ardana push --dry-run began with {first:?}"
        );
        let listed: BTreeMap<String, u64> = lines
            .skip(1)
            .map_while(
                |line| match line.split_whitespace().collect::<Vec<_>>()[..] {
                    [file, bytes] => Some((file.to_string(), bytes.parse().ok()?)),
                    _ => None,
                },
            )
            .collect();
        let mut on_disk = BTreeMap::new();
        for entry in std::fs::read_dir(self.home.join("models").join(name))? {
            let entry = entry?;
            let file = entry.file_name().to_string_lossy().into_owned();
            if file != "finetune.json" {
                on_disk.insert(file, entry.metadata()?.len());
            }
        }
        on_disk.insert("README.md".to_string(), card.len() as u64);
        ensure!(
            listed == on_disk,
            "ardana push --dry-run listed {listed:?}, the directory holds {on_disk:?}"
        );
        println!(
            "e2e finetune {}: ardana push --dry-run lists {} files of {} bytes as they are on disk, the card's included",
            run.row,
            listed.len(),
            listed.values().sum::<u64>()
        );
        Ok(())
    }

    /// What `run`'s model is checked against, as the base names it: a library size by its library entry; an `hf.co/` GGUF
    /// reference by its card (`README.md` in the hub cache), whose one `base_model` is the checkpoint, its snapshot the
    /// one in the cache holding `config.json`; an `hf.co/` safetensors checkpoint by itself, that snapshot; a local GGUF
    /// file by `--checkpoint <dir>`. A `--format safetensors` run expects no quant.
    fn expected(&self, run: &Run) -> Result<Expected> {
        let library = Library::from_file(&self.sandbox.snapshot())?;
        let quant = run.flag("--quant").map(str::to_string);
        let writes_gguf = run.format() == "gguf";
        if let Some(pick) = library.find(&run.base)? {
            // The tokenizer the size pins, else its GGUF repository's, as `ardana pull` reads it.
            let (repo, commit) = match &pick.size.tokenizer {
                Some(pin) => (&pin.repo, &pin.commit),
                None => (&pick.size.gguf.repo, &pick.size.gguf.commit),
            };
            let tokenizer = hub::dir(self.sandbox)
                .join(repo_dir(repo.trim_start_matches("hf.co/")))
                .join("snapshots")
                .join(commit);
            let layout = pick.size.layout.map_or("plain", |layout| match layout {
                ardana_registry::LayoutKind::Plain => "plain",
                ardana_registry::LayoutKind::Chat => "chat",
            });
            let checkpoint = pick.size.source.as_ref().or(pick.size.tokenizer.as_ref());
            let config = checkpoint.map_or_else(PathBuf::new, |pin| {
                hub::dir(self.sandbox)
                    .join(repo_dir(pin.repo.trim_start_matches("hf.co/")))
                    .join("snapshots")
                    .join(&pin.commit)
                    .join("config.json")
            });
            return Ok(Expected {
                checkpoint: json!(checkpoint),
                config,
                quant: writes_gguf.then(|| quant.unwrap_or_else(|| pick.quant.quant.clone())),
                tokenizer,
                layout,
                license: json!(pick.size.license),
                pick: Some(pick),
            });
        }
        let hub = hub::dir(self.sandbox);
        let of_file = |file: &str| {
            FILE_TYPES
                .iter()
                .find(|(quant, _)| matches_quant(file, quant))
                .map(|(quant, _)| quant.to_string())
                .with_context(|| format!("{file} is in no quant the suite checks"))
        };
        // The directory of the base's weights, the checkpoint, the quant and the license.
        let (beside, checkpoint, quant, license) = match Ref::parse(&run.base)? {
            Ref::Hf {
                org,
                repo,
                file: Some(file),
            } => {
                let gguf = cached_gguf(&hub, &org, &repo, &file)?;
                let path = gguf.with_file_name("README.md");
                let card = card(&path)?;
                let [checkpoint] = &card.base_models[..] else {
                    bail!(
                        "{} names the base models {:?}",
                        path.display(),
                        card.base_models
                    );
                };
                let quant = match (&quant, &file) {
                    (Some(quant), _) => quant.clone(),
                    (None, HfFile::Quant(named)) => named.to_ascii_lowercase(),
                    (None, HfFile::Name(name)) => of_file(name)?,
                };
                let license = card.license.clone();
                (
                    gguf.parent().context("the GGUF's directory")?.to_path_buf(),
                    cached_checkpoint(&hub, checkpoint)?,
                    writes_gguf.then_some(quant),
                    license,
                )
            }
            // A safetensors checkpoint: itself, with its own tokenizer, card and layout.
            Ref::Hf {
                org,
                repo,
                file: None,
            } => {
                let (checkpoint, dir) = cached_checkpoint(&hub, &format!("{org}/{repo}"))?;
                let license = card(&dir.join("README.md"))?.license;
                // Q24: a base without a GGUF of its own is quantized to q4_k_m.
                let quant = quant.unwrap_or_else(|| "q4_k_m".into());
                (
                    dir.clone(),
                    (checkpoint, dir),
                    writes_gguf.then_some(quant),
                    license,
                )
            }
            Ref::Local(gguf) => {
                let dir = PathBuf::from(
                    run.flag("--checkpoint")
                        .context("a local GGUF needs --checkpoint")?,
                );
                let name = gguf
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let quant = match quant {
                    Some(quant) => quant,
                    None => of_file(&name)?,
                };
                let license = card(&dir.join("README.md"))?.license;
                (
                    gguf.parent().context("the GGUF's directory")?.to_path_buf(),
                    (json!({"path": dir}), dir),
                    writes_gguf.then_some(quant),
                    license,
                )
            }
            _ => {
                bail!("the suite checks library sizes, hf.co/ references and local GGUF files")
            }
        };
        let (checkpoint, dir) = checkpoint;
        let has = |file: &str| beside.join(file).is_file();
        let layout = if has("decider_config.json") {
            "plain"
        } else {
            "chat"
        };
        Ok(Expected {
            checkpoint,
            quant,
            config: dir.join("config.json"),
            // The weights' own tokenizer, else the checkpoint's.
            tokenizer: if has("tokenizer.json") { beside } else { dir },
            layout,
            license: json!(license),
            pick: None,
        })
    }

    /// R1.1, R3.2, R4.1: the model's entry and its directory: the GGUF in the expected quant ([`Cli::gguf`]) or the
    /// checkpoint ([`Cli::checkpoint`]), the base's tokenizer and chat template files as they are, `decider_config.json`
    /// in the base's layout and `finetune.json` as the Contract has it, its checkpoint the one `expected` names. Returns
    /// that `finetune.json`.
    fn model(&self, run: &Run, expected: &Expected) -> Result<Value> {
        let name = run.name.as_str();
        ensure!(self.listed(name)?, "ardana list does not list {name}");
        let entry: Value =
            serde_json::from_str(&run_stdout(self.command().args(["show", name, "--json"]))?)?;
        let dir = self.home.join("models").join(name);
        let mut files = match &expected.quant {
            Some(quant) => self.gguf(name, quant, &entry, &dir)?,
            None => self.checkpoint(name, expected, &entry, &dir)?,
        };
        for file in [
            "tokenizer.json",
            "tokenizer_config.json",
            "chat_template.jinja",
        ] {
            let base = expected.tokenizer.join(file);
            if base.is_file() {
                ensure!(
                    std::fs::read(dir.join(file)).ok() == std::fs::read(&base).ok(),
                    "{name}'s {file} is not the base's"
                );
                files.push(file.to_string());
            }
        }
        let config = read_json(&dir.join("decider_config.json"))?;
        let layout = expected.layout;
        ensure!(
            config["layout"] == layout && entry["profile"]["layout"]["kind"] == layout,
            "{name}'s decider_config.json is {config}, its profile {}, not in the {layout} layout",
            entry["profile"]
        );
        let record = read_json(&dir.join("finetune.json"))?;
        // Every line of the row's data is labelled: a tenth to calibrate on, a tenth to test on, the rest to train on.
        let lines = self.lines(run)?.len();
        let tenth = lines / 10;
        let mut written = record.clone();
        let report = written["report"].take();
        ensure!(
            report["lines"] == tenth,
            "{name}'s finetune.json reports {report}, not the {tenth} test lines"
        );
        let wanted = json!({
            "base": run.base,
            "checkpoint": expected.checkpoint,
            "format": run.format(),
            "quant": expected.quant,
            "device": "cpu",
            "dtype": "float32",
            "qlora": false,
            "epochs": run.epochs()?,
            "rows": {"train": lines - 2 * tenth, "calibration": tenth, "test": tenth},
            "license": expected.license,
            "report": null,
        });
        ensure!(
            written == wanted,
            "{name}'s finetune.json is {written}, not {wanted}"
        );
        files.extend(["decider_config.json".into(), "finetune.json".into()]);
        let mut held: Vec<String> = std::fs::read_dir(&dir)?
            .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<std::io::Result<_>>()?;
        held.sort();
        files.sort();
        ensure!(
            held == files,
            "{} holds {held:?}, not {files:?}",
            dir.display()
        );
        println!(
            "e2e finetune {}: {} holds {}; finetune.json {written}",
            run.row,
            dir.display(),
            held.join(", ")
        );
        Ok(record)
    }

    /// R1.1: `name` is recorded with llama.cpp as its runtime and `<name>-<QUANT>.gguf` in `dir` as its weights, a GGUF of
    /// `quant`'s file type. Returns the file's name.
    fn gguf(&self, name: &str, quant: &str, entry: &Value, dir: &Path) -> Result<Vec<String>> {
        let gguf = dir.join(format!("{name}-{}.gguf", quant.to_ascii_uppercase()));
        ensure!(
            entry["runtime"] == "llama.cpp"
                && Path::new(entry["weights"].as_str().unwrap_or_default()) == gguf,
            "{name} is recorded as {entry}"
        );
        let file_type = gguf_file_type(&gguf)?;
        let wanted = FILE_TYPES
            .iter()
            .find(|(q, _)| *q == quant)
            .map(|(_, t)| *t)
            .with_context(|| format!("no file type for {quant}"))?;
        ensure!(
            file_type == wanted,
            "{} has the file type {file_type}, not {quant}'s {wanted}",
            gguf.display()
        );
        Ok(vec![
            gguf.file_name().unwrap().to_string_lossy().into_owned(),
        ])
    }

    /// R4.1: `name` is recorded with Candle as its runtime, which `ardana show` prints, and `dir` itself as its weights:
    /// the checkpoint's `config.json` (its text tower's for a multimodal one) with the dtype float32, and the
    /// safetensors its index names (else `model.safetensors`), every tensor in F32. Returns those files' names.
    fn checkpoint(
        &self,
        name: &str,
        expected: &Expected,
        entry: &Value,
        dir: &Path,
    ) -> Result<Vec<String>> {
        ensure!(
            entry["runtime"] == "candle"
                && Path::new(entry["weights"].as_str().unwrap_or_default()) == dir,
            "{name} is recorded as {entry}"
        );
        let shown = run_stdout(self.command().args(["show", name]))?;
        ensure!(
            shown
                .lines()
                .any(|line| line.split_whitespace().eq(["runtime", "candle"])),
            "ardana show {name} prints no runtime candle:\n{shown}"
        );
        let mut wanted = read_json(&expected.config)?;
        if let Some(text) = wanted.get("text_config").cloned() {
            let architecture = wanted["architectures"][0]
                .as_str()
                .unwrap_or_default()
                .replace("ForConditionalGeneration", "ForCausalLM");
            wanted = text;
            wanted["architectures"] = json!([architecture]);
        }
        for key in ["dtype", "torch_dtype"] {
            if wanted.get(key).is_some() {
                wanted[key] = json!("float32");
            }
        }
        let config = read_json(&dir.join("config.json"))?;
        ensure!(
            config == wanted,
            "{name}'s config.json is {config}, not the checkpoint's {wanted}"
        );
        let mut files = vec!["config.json".to_string()];
        let index = dir.join("model.safetensors.index.json");
        let weights: BTreeSet<String> = if index.is_file() {
            files.push("model.safetensors.index.json".into());
            read_json(&index)?["weight_map"]
                .as_object()
                .context("the index's weight_map")?
                .values()
                .filter_map(|file| file.as_str().map(str::to_string))
                .collect()
        } else {
            BTreeSet::from(["model.safetensors".to_string()])
        };
        for file in &weights {
            let dtypes = safetensors_dtypes(&dir.join(file))?;
            ensure!(
                dtypes.iter().all(|dtype| dtype == "F32"),
                "{name}'s {file} holds the dtypes {dtypes:?}, not F32 alone"
            );
        }
        println!(
            "e2e finetune: {name} runs through candle, its config.json the checkpoint's in float32 ({}), {} in F32",
            config["architectures"],
            weights.iter().cloned().collect::<Vec<_>>().join(", ")
        );
        files.extend(weights);
        Ok(files)
    }

    /// The lines of a run's data as JSONL lines: the CSV of the commands data holds the commands data's lines.
    fn lines(&self, run: &Run) -> Result<Vec<Value>> {
        let data = match run.data()? {
            COMMANDS_CSV => COMMANDS,
            data => data,
        };
        std::fs::read_to_string(self.dir.join(data))?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Ok(serde_json::from_str(line)?))
            .collect()
    }

    /// The request a row's model answers: the first line of the row's data, its state with its own questions, else
    /// those of the row's `--questions` file.
    fn request(&self, run: &Run) -> Result<Value> {
        let lines = self.lines(run)?;
        let line = lines.first().context("the data is empty")?;
        let questions = match (line.get("questions"), run.flag("--questions")) {
            (Some(own), _) => own.clone(),
            (None, Some(file)) => read_json(&self.dir.join(file))?,
            (None, None) => bail!("{}'s first line asks no questions", run.data()?),
        };
        Ok(json!({"state": line["state"], "questions": questions}))
    }

    /// R1.1: `ardana run <name> --request -` answers every question of `request`, read from stdin, and its `--json`
    /// gives each question an answer of its type; returns that response.
    fn answered(&self, name: &str, request: &Value) -> Result<Value> {
        let text = request.to_string();
        let mut printed = String::new();
        for json in [false, true] {
            let mut cmd = self.command();
            cmd.args(["run", name, "--request", "-"]);
            if json {
                cmd.arg("--json");
            }
            let mut child = cmd
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            std::io::Write::write_all(&mut child.stdin.take().context("stdin")?, text.as_bytes())?;
            let output = child.wait_with_output()?;
            ensure!(
                output.status.success(),
                "ardana run {name} --request - failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            printed = String::from_utf8(output.stdout)?;
            if !json {
                println!(
                    "e2e finetune: ardana run {name} --request - printed:\n{}",
                    printed.trim_end()
                );
            }
        }
        let response: Value = serde_json::from_str(&printed)?;
        for (id, question) in request["questions"].as_object().context("the questions")? {
            ensure!(
                response["answers"][id]["type"] == question["type"],
                "{name} answered {id} with {}",
                response["answers"][id]
            );
        }
        Ok(response)
    }

    /// R2.4: the server, running since the suite started, lists `name` as pulled in `/v1/models` and answers `request`
    /// with it at `/v1/systemone` as `ardana run --json` answered it (`response`), every probability the same.
    fn served(&self, server: &Server, name: &str, request: &Value, response: &Value) -> Result<()> {
        let curl = |args: &[&str]| -> Result<Value> {
            let output = self
                .sandbox
                .command("curl")
                .args(["-fsS", "--max-time", "600"])
                .args(args)
                .output()
                .context("running curl")?;
            ensure!(
                output.status.success(),
                "the server did not answer: {}\n{}",
                String::from_utf8_lossy(&output.stderr).trim(),
                server.log_text()
            );
            Ok(serde_json::from_slice(&output.stdout)?)
        };
        let models = curl(&[&format!("{}/v1/models", server.url)])?;
        let listed = models["models"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|model| model["name"] == name);
        ensure!(
            listed.is_some_and(|model| model["x_pulled"] != false),
            "the server's /v1/models lists {name} as {listed:?}: {models}"
        );
        let mut body = request.clone();
        body["model"] = name.into();
        let served = curl(&[
            "-H",
            "content-type: application/json",
            "--data-binary",
            &body.to_string(),
            &format!("{}/v1/systemone", server.url),
        ])?;
        ensure!(
            served["answers"] == response["answers"] && served["model"] == response["model"],
            "the server answered {served}, ardana run --json {response}"
        );
        println!(
            "e2e finetune: the server running since the suite started lists {name} and answers with it as `ardana run` does"
        );
        Ok(())
    }

    /// R2.1, R4.1: the temperatures the run fitted, through the runtime its model is recorded with (Q11: llama.cpp for a
    /// GGUF, Candle for a checkpoint), as its stderr says them (`calibrated <name> through <runtime>`, the id of the
    /// runtime that loaded the model, then `temperature T over N calibration answers` and `; <type> T over n` or `;
    /// <type> takes it (n, fewer than 50)` per type), follow Q30, are those of the model's `decider_config.json`, and
    /// `ardana show <name> --json` prints them in its profile.
    fn calibrated(&self, run: &Run, log: &str) -> Result<()> {
        let name = run.name.as_str();
        let runtime = match run.format() {
            "safetensors" => "candle",
            _ => "llama.cpp",
        };
        let through = format!("calibrated {name} through {runtime}");
        ensure!(
            log.lines().any(|line| line == through),
            "the run does not say `{through}`:\n{}",
            tail(log)
        );
        let said = log
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name} answers at temperature ")))
            .context("the run says no fitted temperatures")?;
        let mut parts = said.split("; ");
        let overall = parts.next().unwrap_or_default();
        let temperature: f64 = overall
            .split_once(' ')
            .map_or(overall, |(t, _)| t)
            .parse()
            .with_context(|| format!("the temperature in `{said}`"))?;
        let mut by_type = serde_json::Map::new();
        for part in parts {
            let words: Vec<&str> = part.split_whitespace().collect();
            match words[..] {
                [kind, t, "over", n] => {
                    ensure!(
                        n.parse::<usize>()? >= 50,
                        "{kind} has its own temperature over {n} answers"
                    );
                    by_type.insert(kind.to_string(), json!(t.parse::<f64>()?));
                }
                [kind, "takes", "it", n, ..] => {
                    let n: usize = n.trim_start_matches('(').trim_end_matches(',').parse()?;
                    ensure!(
                        n < 50,
                        "{kind} takes the overall temperature over {n} answers"
                    );
                }
                _ => bail!("the run says `{part}` of a type's temperature"),
            }
        }
        let config = read_json(
            &self
                .home
                .join("models")
                .join(name)
                .join("decider_config.json"),
        )?;
        let by_type = Value::Object(by_type);
        ensure!(
            config["temperature"] == json!(temperature) && config["temperature_by_type"] == by_type,
            "{name}'s decider_config.json is {config}, the run said `{said}`"
        );
        let entry: Value =
            serde_json::from_str(&run_stdout(self.command().args(["show", name, "--json"]))?)?;
        let profile = &entry["profile"];
        ensure!(
            profile["temperature"] == config["temperature"]
                && profile["temperature_by_type"] == config["temperature_by_type"],
            "ardana show {name} --json prints the profile {profile}, its decider_config.json is {config}"
        );
        println!(
            "e2e finetune {}: {through}; {name} answers at temperature {said}",
            run.row
        );
        Ok(())
    }

    /// R2.2 and R2.3: `finetune.json#report` holds, for the base (a library size pulled under its canonical name from
    /// its default quant, as `ardana run <base>` pulls it; any other base under the name the run gives it) and for the
    /// model, the answers, accuracy, NLL, Brier score and ECE of each answer type and overall on the test split, the
    /// table the run printed on stdout gives the same numbers, and the model's overall accuracy is as the row's
    /// [`Gain`] expects against the base's.
    fn reported(&self, run: &Run, expected: &Expected, printed: &str) -> Result<()> {
        let name = run.name.as_str();
        let tuned: Value =
            serde_json::from_str(&run_stdout(self.command().args(["show", name, "--json"]))?)?;
        let base = match &expected.pick {
            Some(pick) => {
                let base = pick.name();
                let entry: Value = serde_json::from_str(&run_stdout(
                    self.command().args(["show", &base, "--json"]),
                )?)?;
                ensure!(
                    entry["source"] == pick.reference(),
                    "the base {base} is pulled from {}, not from its default quant {}",
                    entry["source"],
                    pick.reference()
                );
                // Q7: the model reads its rows as the base does, in its layout and score-level mode.
                for key in ["layout", "isolated_levels"] {
                    let (base_mode, tuned_mode) = (&entry["profile"][key], &tuned["profile"][key]);
                    ensure!(
                        base_mode == tuned_mode
                            || (key == "layout" && base_mode["kind"] == tuned_mode["kind"]),
                        "{name} reads {key} {tuned_mode}, the base {base_mode}"
                    );
                }
                base
            }
            None => {
                // Q7: a base pulled as a reference is recorded nowhere; the model keeps the layout it is read in.
                ensure!(
                    tuned["profile"]["layout"]["kind"] == expected.layout,
                    "{name} reads the layout {}, the base {}",
                    tuned["profile"]["layout"],
                    expected.layout
                );
                run.base.clone()
            }
        };
        let record = read_json(&self.home.join("models").join(name).join("finetune.json"))?;
        let report = &record["report"];
        ensure!(
            report["base"]["name"] == base && report["tuned"]["name"] == name,
            "{name}'s report compares {} with {}",
            report["tuned"]["name"],
            report["base"]["name"]
        );
        // Every row of the table, keyed by type and model, as the report holds it.
        let mut table: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for line in printed
            .lines()
            .skip_while(|line| !line.starts_with("TYPE "))
            .skip(1)
        {
            let cells: Vec<&str> = line.split_whitespace().collect();
            if let [kind, model, numbers @ ..] = &cells[..]
                && numbers.len() == 5
            {
                table.insert(
                    (kind.to_string(), model.to_string()),
                    numbers.iter().map(|n| n.to_string()).collect(),
                );
            }
        }
        let mut accuracy = BTreeMap::new();
        for side in ["base", "tuned"] {
            let metrics = report[side]["metrics"]
                .as_object()
                .with_context(|| format!("{name}'s report has no {side} metrics: {report}"))?;
            let keys: Vec<&String> = metrics.keys().collect();
            ensure!(
                keys.last().is_some_and(|key| *key == "overall")
                    && keys[..keys.len() - 1]
                        .iter()
                        .all(|key| ["choice", "noul", "score"].contains(&key.as_str())),
                "{name}'s {side} report has the keys {keys:?}"
            );
            let model = report[side]["name"].as_str().unwrap_or_default();
            for (kind, m) in metrics {
                let number = |key: &str| {
                    m[key]
                        .as_f64()
                        .with_context(|| format!("{side} {kind} {key}: {m}"))
                };
                let answers = m["answers"].as_u64().context("the answers")?;
                let (acc, nll, brier, ece) = (
                    number("accuracy")?,
                    number("nll")?,
                    number("brier")?,
                    number("ece")?,
                );
                ensure!(
                    answers > 0
                        && (0.0..=1.0).contains(&acc)
                        && nll >= 0.0
                        && (0.0..=2.0).contains(&brier)
                        && (0.0..=1.0).contains(&ece),
                    "{name}'s {side} {kind} metrics are {m}"
                );
                ensure!(
                    m["answers"] == report["base"]["metrics"][kind]["answers"],
                    "the base and {name} answered different {kind} questions: {report}"
                );
                let wanted: Vec<String> = std::iter::once(answers.to_string())
                    .chain([acc, nll, brier, ece].iter().map(|x| format!("{x:.4}")))
                    .collect();
                let row = table.get(&(kind.clone(), model.to_string()));
                ensure!(
                    row == Some(&wanted),
                    "the run printed {row:?} for {model}'s {kind}, its report holds {wanted:?}:\n{printed}"
                );
                if kind == "overall" {
                    accuracy.insert(side, acc);
                }
            }
        }
        let (before, after) = (accuracy["base"], accuracy["tuned"]);
        match run.gain {
            Gain::Above => ensure!(
                after > before,
                "{name}'s test accuracy {after} is not above the base's {before}"
            ),
            Gain::Within(drop) => ensure!(
                after >= before - drop,
                "{name}'s test accuracy {after} is more than {drop} below the base's {before}"
            ),
            Gain::Unchecked => {}
        }
        println!(
            "e2e finetune {}: test accuracy {before} for {base}, {after} for {name}; the run printed\n{}",
            run.row,
            printed.trim_end()
        );
        Ok(())
    }

    /// Every file and link of the training environment but the runs' working directories, with its size and
    /// modification time.
    fn environment_files(&self) -> Result<Files> {
        let root = self.home.join("finetune");
        let mut files = Files::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in
                std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let meta = entry.metadata()?;
                if meta.is_dir() {
                    if path != root.join("work") {
                        dirs.push(path);
                    }
                    continue;
                }
                let rel = path.strip_prefix(&root)?.to_path_buf();
                files.insert(rel, (meta.len(), meta.modified()?));
            }
        }
        Ok(files)
    }

    /// R2.5: a row after the first that answers reused the training environment: no run of the row downloaded or
    /// built a part of it, and every file of it is as the first row left it.
    fn reused(&self, run: &Run, first: &Files, files: &Files) -> Result<()> {
        let name = &run.name;
        // The row's runs: the three it always makes, then the untrusted and the local one when it makes them.
        for (log, always) in [
            ("-interrupted", true),
            ("-failing", true),
            ("", true),
            ("-untrusted", false),
            ("-local", false),
        ] {
            let log = self.dir.join(format!("{name}{log}.log"));
            if !always && !log.exists() {
                continue;
            }
            let text = std::fs::read_to_string(&log)?;
            if let Some(line) = text.lines().find(|line| {
                ENVIRONMENT_BUILT
                    .iter()
                    .any(|said| line.trim_start().starts_with(said))
            }) {
                bail!("{} says `{line}`", log.display());
            }
        }
        let changed: Vec<&PathBuf> = first
            .iter()
            .filter(|(path, meta)| files.get(*path) != Some(meta))
            .map(|(path, _)| path)
            .chain(files.keys().filter(|path| !first.contains_key(*path)))
            .collect();
        ensure!(
            changed.is_empty(),
            "the training environment changed since the first row: {} files, {:?}",
            changed.len(),
            &changed[..changed.len().min(10)]
        );
        println!(
            "e2e finetune {}: reused uv, Python, the venv and the converter: nothing downloaded, the {} files unchanged",
            run.row,
            files.len()
        );
        Ok(())
    }

    /// `ardana rm` of every entry listed now but not in `before` (the models `names`, a base the row pulled), and the
    /// model directory of each of `names` that is left.
    fn remove(&self, names: &[&str], before: &[String]) -> Result<()> {
        let added: Vec<String> = self
            .entries()?
            .into_iter()
            .filter(|entry| !before.contains(entry))
            .collect();
        if !added.is_empty() {
            run_stdout(self.command().arg("rm").args(&added))?;
        }
        for name in names {
            let dir = self.home.join("models").join(name);
            if dir.exists() {
                std::fs::remove_dir_all(&dir)
                    .with_context(|| format!("removing {}", dir.display()))?;
            }
        }
        Ok(())
    }
}

/// What a run's model is checked against ([`Cli::expected`]).
struct Expected {
    /// `finetune.json#checkpoint`.
    checkpoint: Value,
    /// The GGUF's quant; none for `--format safetensors`.
    quant: Option<String>,
    /// The checkpoint's `config.json`.
    config: PathBuf,
    /// The directory of the tokenizer files the model copies.
    tokenizer: PathBuf,
    layout: &'static str,
    /// `finetune.json#license`.
    license: Value,
    /// The library size the base is, whose entry the report reads.
    pick: Option<LibraryPick>,
}

/// The card at `path`, read as `ardana finetune` reads one.
fn card(path: &Path) -> Result<Card> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(Card::parse(&text))
}

/// The dtypes of the tensors of the safetensors file at `path`, from its header: a little-endian `u64` length, then that
/// many bytes of JSON naming every tensor (and `__metadata__`).
fn safetensors_dtypes(path: &Path) -> Result<BTreeSet<String>> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut len = [0; 8];
    file.read_exact(&mut len)?;
    let mut header = vec![0; usize::try_from(u64::from_le_bytes(len))?];
    file.read_exact(&mut header)?;
    let header: Value =
        serde_json::from_slice(&header).with_context(|| format!("{}'s header", path.display()))?;
    Ok(header
        .as_object()
        .context("a header object")?
        .iter()
        .filter(|(key, _)| *key != "__metadata__")
        .filter_map(|(_, tensor)| tensor["dtype"].as_str().map(str::to_string))
        .collect())
}

/// The `general.file_type` of the GGUF at `path`, read from its key-value header (GGUF v3).
fn gguf_file_type(path: &Path) -> Result<u64> {
    let mut file = BufReader::new(
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?,
    );
    let mut take = |n: usize| -> Result<Vec<u8>> {
        let mut bytes = vec![0; n];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    };
    let mut magic = take(8)?;
    ensure!(
        magic.drain(..4).as_slice() == b"GGUF",
        "{} is no GGUF",
        path.display()
    );
    let u64_of = |bytes: Vec<u8>| u64::from_le_bytes(bytes.try_into().expect("eight bytes"));
    let u32_of = |bytes: Vec<u8>| u32::from_le_bytes(bytes.try_into().expect("four bytes"));
    let _tensors = u64_of(take(8)?);
    let pairs = u64_of(take(8)?);
    for _ in 0..pairs {
        let len = u64_of(take(8)?) as usize;
        let key = String::from_utf8(take(len)?)?;
        let kind = u32_of(take(4)?);
        if key == "general.file_type" {
            return Ok(match kind {
                4 => u64::from(u32_of(take(4)?)),
                10 => u64_of(take(8)?),
                other => bail!("general.file_type has the type {other}"),
            });
        }
        skip_value(&mut take, kind)?;
    }
    bail!("{} has no general.file_type", path.display())
}

/// Reads past one GGUF value of type `kind`.
fn skip_value(take: &mut impl FnMut(usize) -> Result<Vec<u8>>, kind: u32) -> Result<()> {
    let len = |take: &mut dyn FnMut(usize) -> Result<Vec<u8>>| -> Result<usize> {
        Ok(u64::from_le_bytes(take(8)?.try_into().expect("eight bytes")) as usize)
    };
    match kind {
        0 | 1 | 7 => drop(take(1)?),
        2 | 3 => drop(take(2)?),
        4..=6 => drop(take(4)?),
        10..=12 => drop(take(8)?),
        8 => {
            let n = len(take)?;
            take(n)?;
        }
        9 => {
            let element = u32::from_le_bytes(take(4)?.try_into().expect("four bytes"));
            for _ in 0..len(take)? {
                skip_value(take, element)?;
            }
        }
        other => bail!("a GGUF value of the unknown type {other}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interrupted run waits for the trainer's own bar past its first step: not the bars that load weights or
    /// data, and not the bars before ardana says it trains.
    #[test]
    fn training_waits_for_a_step() {
        let loading = "Loading weights:   0%|          | 1/311 [00:00<00:53,  5.76it/s]\r";
        assert!(!training(loading));
        let before =
            format!("{loading}\ntraining cmd-risk from qwen3:0.6b on cpu: 1600 rows, 2 epochs\n");
        assert!(!training(&before));
        let started = format!("{before}{loading}  0%|          | 0/400 [00:00<?, ?it/s]");
        assert!(!training(&started));
        assert!(training(&format!(
            "{started}\r  0%|          | 1/400 [00:16<1:46:00, 16.0s/it]"
        )));
    }
}
