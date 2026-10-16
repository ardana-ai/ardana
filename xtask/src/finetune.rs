//! `cargo xtask e2e finetune`: each row of [`crate::e2e::FINETUNE_FIXTURES`] in turn, with the release `ardana`, online,
//! on one suite home, `tmp/e2e/finetune/home`, over the sandbox's hub cache. The suite first writes the data the rows
//! name into its directory: the first 1,000 lines of decider@23579f7's `teacher_data/commands.jsonl` (Q22) as lines of
//! `ardana finetune`'s format, their two shared questions in a questions file. A row that answers fine-tunes its base
//! three times: stopped by SIGINT mid-training, then with a trainer that exits non-zero (its script moved out of the
//! venv and back), neither leaving an entry or a model directory, then to the end, and its model answers through
//! `ardana run` before `ardana rm` removes it; a refused row's run exits 1 naming why, before any weight downloads.
//! Whatever a row's checks found, its entries, model directories and the hub paths it added are removed when they
//! end; the training environment in the home stays for the next row, and the suite removes the home when it ends,
//! also when a row failed.

use std::collections::BTreeSet;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::e2e::{Expect, FinetuneFixture, read_json};
use crate::hub;
use crate::sandbox::{Sandbox, run_stdout};
use crate::serve::build_ardana;

/// The commands data, in the suite's directory: its lines, and their shared questions.
pub const COMMANDS: &str = "commands.jsonl";
pub const COMMANDS_QUESTIONS: &str = "commands.questions.json";
/// The teacher data of the decider clone `cargo xtask fetch` makes (decider@23579f7, Apache-2.0).
const TEACHER_COMMANDS: &str = "src/decider/teacher_data/commands.jsonl";
/// The lines of it the commands data takes, and the ids its two questions get (the risk choice, then the noul).
const COMMANDS_LINES: usize = 1000;
const QUESTION_IDS: [&str; 2] = ["risk", "outside"];
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
const FILE_TYPES: [(&str, u64); 2] = [("q8_0", 7), ("q4_k_m", 15)];
/// How long the interrupted run may take to reach its first training step (the environment and the checkpoint
/// download first), and to exit once interrupted.
const FIRST_STEP: Duration = Duration::from_secs(3600);
const STOPPING: Duration = Duration::from_secs(120);

/// Runs every row, a failed one counted but not ending the suite, then removes the suite's directory.
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
    match commands_data(sandbox, &dir) {
        Ok(()) => {
            for fixture in fixtures {
                let started = Instant::now();
                match row(&cli, fixture) {
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
        }
        Err(err) => {
            println!("e2e finetune: writing the data failed: {err:#}");
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
/// "answers"}`, its two questions, which every line shares, in [`COMMANDS_QUESTIONS`] under [`QUESTION_IDS`].
fn commands_data(sandbox: &Sandbox, dir: &Path) -> Result<()> {
    let source = sandbox.tmp().join(TEACHER_COMMANDS);
    let text = std::fs::read_to_string(&source)
        .with_context(|| format!("reading {}; run `cargo xtask fetch`", source.display()))?;
    let mut shared: Option<Value> = None;
    let mut lines = String::new();
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
        lines.push_str(&json!({"state": teacher["state"], "answers": answers}).to_string());
        lines.push('\n');
    }
    std::fs::write(dir.join(COMMANDS), lines)?;
    let shared = shared.context("the teacher data is empty")?;
    std::fs::write(
        dir.join(COMMANDS_QUESTIONS),
        serde_json::to_string_pretty(&shared)?,
    )?;
    println!(
        "e2e finetune: wrote {COMMANDS} ({COMMANDS_LINES} lines of {}) and {COMMANDS_QUESTIONS}",
        source.display()
    );
    Ok(())
}

/// One row: its checks, then, whatever they found, the entry and model directory of the name it fine-tunes and every
/// path it added to the hub cache removed.
fn row(cli: &Cli, fixture: &FinetuneFixture) -> Result<()> {
    let hub = hub::dir(cli.sandbox);
    let before = hub::paths(&hub)?;
    let name = flag(fixture, "--name").context("the row names no --name")?;
    let checked = match fixture.expect {
        Expect::Answers => answers(cli, fixture, name),
        Expect::Refused(why) => refused(cli, fixture, name, why, &before),
    };
    let removed = cli
        .remove(name)
        .and(hub::remove_added(&hub, &before))
        .context("removing what the row added");
    match &removed {
        Ok(bytes) => println!(
            "e2e finetune {}: removed {name} and {:.2} GB from {}",
            fixture.name,
            *bytes as f64 / 1e9,
            hub.display()
        ),
        Err(err) => println!("e2e finetune {}: {err:#}", fixture.name),
    }
    checked.and(removed.map(drop))
}

/// R1.1, R1.2 and R1.7 for a row that answers: a run stopped by SIGINT mid-training and a run whose trainer exits
/// non-zero leave no entry and no model directory, the first having installed the environment; then the fine-tune
/// runs to the end, its directory holds the GGUF in the expected quant, the tokenizer and chat template files of the
/// base, `decider_config.json` in the base's layout and `finetune.json`, `ardana run` answers every question with it,
/// and `ardana rm` removes the entry and the directory.
fn answers(cli: &Cli, fixture: &FinetuneFixture, name: &str) -> Result<()> {
    let row = fixture.name;
    let log = cli.dir.join(format!("{name}-interrupted.log"));
    let started = Instant::now();
    let mut child = cli.spawn(fixture, &log)?;
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
        .spawn(fixture, &log)
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

    let log = cli.dir.join(format!("{name}.log"));
    let started = Instant::now();
    let status = cli.spawn(fixture, &log)?.wait()?;
    let text = std::fs::read_to_string(&log)?;
    ensure!(
        status.success(),
        "the fine-tune exited {status}:\n{}",
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
    let epochs = epochs(fixture)?;
    ensure!(
        evaluated.len() as u64 == epochs,
        "the trainer evaluated the calibration split {} times in {epochs} epochs",
        evaluated.len()
    );
    for metrics in evaluated {
        println!("e2e finetune {row}: {metrics}");
    }
    cli.model(fixture, name)?;
    cli.answered(name)?;
    let removed = run_stdout(cli.command().args(["rm", name]))?;
    ensure!(
        !cli.home.join("models").join(name).exists() && !cli.listed(name)?,
        "ardana rm {name} left its entry or its directory: {removed}"
    );
    println!("e2e finetune {row}: {}", removed.trim());
    Ok(())
}

/// The value a row's args give `flag`.
fn flag(fixture: &FinetuneFixture, flag: &str) -> Option<&'static str> {
    fixture
        .args
        .windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1])
}

/// The epochs a row trains: its `--epochs`, else `ardana finetune`'s default (Q23).
fn epochs(fixture: &FinetuneFixture) -> Result<u64> {
    Ok(flag(fixture, "--epochs").map_or(Ok(2), str::parse)?)
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
            bail!("the run exited ({status}) before its first training step");
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

/// A refused row: `ardana finetune` exits 1 with `why` in its message, before any weight download (no file the hub
/// cache gained since `before` is a weight file), and leaves no entry and no model directory.
fn refused(
    cli: &Cli,
    fixture: &FinetuneFixture,
    name: &str,
    why: &str,
    before: &BTreeSet<PathBuf>,
) -> Result<()> {
    let log = cli.dir.join(format!("{name}.log"));
    let status = cli.spawn(fixture, &log)?.wait()?;
    let text = std::fs::read_to_string(&log)?;
    ensure!(
        status.code() == Some(1) && text.contains(why),
        "ardana finetune {} exited {status}, not refusing it as `{why}`:\n{}",
        fixture.base,
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
    println!("e2e finetune {}: refused, {message}", fixture.name);
    Ok(())
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

    /// `ardana finetune <base> <args>`, its output going to `log`.
    fn spawn(&self, fixture: &FinetuneFixture, log: &Path) -> Result<Child> {
        let file =
            std::fs::File::create(log).with_context(|| format!("creating {}", log.display()))?;
        let mut cmd = self.command();
        cmd.arg("finetune")
            .arg(fixture.base)
            .args(fixture.args)
            .stdin(Stdio::null())
            .stdout(file.try_clone()?)
            .stderr(file);
        cmd.spawn().context("starting ardana finetune")
    }

    /// Whether `ardana list` lists `name`.
    fn listed(&self, name: &str) -> Result<bool> {
        let listed = run_stdout(self.command().arg("list"))?;
        Ok(listed
            .lines()
            .any(|line| line.split_whitespace().next() == Some(name)))
    }

    /// R1.7: no entry named `name`, no model directory and no working directory of a run that builds it.
    fn left_nothing(&self, name: &str) -> Result<()> {
        ensure!(!self.listed(name)?, "the run left the entry {name}");
        for dir in [
            self.home.join("models").join(name),
            self.home.join("finetune/work").join(name),
        ] {
            ensure!(!dir.exists(), "the run left {}", dir.display());
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
        let report = run_stdout(self.sandbox.command(&python).args(["-I", "-c", &check]))?;
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

    /// R1.1: the model's entry and its directory: the GGUF in the quant the run names (else the base's own), the
    /// base's tokenizer and chat template files as they are, `decider_config.json` in the base's layout and
    /// `finetune.json` as the Contract has it.
    fn model(&self, fixture: &FinetuneFixture, name: &str) -> Result<()> {
        ensure!(self.listed(name)?, "ardana list does not list {name}");
        let entry: Value =
            serde_json::from_str(&run_stdout(self.command().args(["show", name, "--json"]))?)?;
        let dir = self.home.join("models").join(name);
        let library = ardana_registry::library::Library::from_file(&self.sandbox.snapshot())?;
        let pick = library
            .find(fixture.base)?
            .with_context(|| format!("{} is no library size", fixture.base))?;
        let quant = flag(fixture, "--quant").unwrap_or(pick.quant.quant.as_str());
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

        let tokenizer = pick
            .size
            .tokenizer
            .as_ref()
            .or(pick.size.source.as_ref())
            .context("the base pins no tokenizer repository")?;
        let repo = tokenizer
            .repo
            .trim_start_matches("hf.co/")
            .replace('/', "--");
        let snapshot = hub::dir(self.sandbox)
            .join(format!("models--{repo}/snapshots"))
            .join(&tokenizer.commit);
        let mut files = vec![gguf.file_name().unwrap().to_string_lossy().into_owned()];
        for file in [
            "tokenizer.json",
            "tokenizer_config.json",
            "chat_template.jinja",
        ] {
            if snapshot.join(file).is_file() {
                ensure!(
                    std::fs::read(dir.join(file)).ok() == std::fs::read(snapshot.join(file)).ok(),
                    "{name}'s {file} is not the base's"
                );
                files.push(file.to_string());
            }
        }
        let config = read_json(&dir.join("decider_config.json"))?;
        let layout = pick.size.layout.map_or("plain", |layout| match layout {
            ardana_registry::LayoutKind::Plain => "plain",
            ardana_registry::LayoutKind::Chat => "chat",
        });
        ensure!(
            config["layout"] == layout && entry["profile"]["layout"]["kind"] == layout,
            "{name}'s decider_config.json is {config}, its profile {}, not in the {layout} layout",
            entry["profile"]
        );
        let record = read_json(&dir.join("finetune.json"))?;
        let checkpoint = pick.size.source.as_ref().or(pick.size.tokenizer.as_ref());
        let epochs = epochs(fixture)?;
        // Every line of the row's data is labelled: a tenth to calibrate on, a tenth to test on, the rest to train on.
        let data = fixture.args.first().context("the row names no data")?;
        let lines = std::fs::read_to_string(self.dir.join(data))?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        let tenth = lines / 10;
        let wanted = json!({
            "base": fixture.base,
            "checkpoint": checkpoint,
            "format": "gguf",
            "quant": quant,
            "device": "cpu",
            "dtype": "float32",
            "qlora": false,
            "epochs": epochs,
            "rows": {"train": lines - 2 * tenth, "calibration": tenth, "test": tenth},
            "license": pick.size.license,
            "report": null,
        });
        ensure!(
            record == wanted,
            "{name}'s finetune.json is {record}, not {wanted}"
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
            "e2e finetune {}: {} holds {}; finetune.json {record}",
            fixture.name,
            dir.display(),
            held.join(", ")
        );
        Ok(())
    }

    /// R1.1: `ardana run <name> --request -` answers every question of a request on the shared questions, read from
    /// stdin, and its `--json` gives each question an answer of its type.
    fn answered(&self, name: &str) -> Result<()> {
        let questions = read_json(&self.dir.join(COMMANDS_QUESTIONS))?;
        let request =
            json!({"state": "git push --force origin main", "questions": questions}).to_string();
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
            std::io::Write::write_all(
                &mut child.stdin.take().context("stdin")?,
                request.as_bytes(),
            )?;
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
        for (id, question) in questions.as_object().context("the questions")? {
            ensure!(
                response["answers"][id]["type"] == question["type"],
                "{name} answered {id} with {}",
                response["answers"][id]
            );
        }
        Ok(())
    }

    /// `ardana rm <name>` when it is listed, and its model directory when one is left.
    fn remove(&self, name: &str) -> Result<()> {
        if self.listed(name)? {
            run_stdout(self.command().args(["rm", name]))?;
        }
        let dir = self.home.join("models").join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
        }
        Ok(())
    }
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
