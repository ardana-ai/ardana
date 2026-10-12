//! `cargo xtask e2e candle`: each row of [`crate::e2e::CANDLE_FIXTURES`] in turn, with the release `ardana`. A row
//! pulls a safetensors checkpoint and the GGUF of the same model from the Hub into `tmp/hf`, checks that Candle
//! answers as llama.cpp does, and removes what it pulled before the next row starts; a row without a GGUF (the THX-01
//! decision model) checks the checkpoint against the `thx01` package's goldens instead. Either way the row's
//! checkpoint answers through `ardana run` and through one `ardana serve`. The suite ends with `cargo clean`. The
//! disk holds neither every row's weights at once nor a row's beside a full `target/`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::e2e::{CandleFixture, Pin, read_json};
use crate::sandbox::{Sandbox, cargo, run, run_stdout};
use crate::serve::{Server, build_ardana};

/// The request fixtures every row answers, in `tests/fixtures/requests`.
const REQUESTS: [&str; 3] = ["ticket.json", "sentiment.json", "score.json"];
/// How far a probability through Candle may be from the GGUF's.
const TOLERANCE: f64 = 0.1;
/// How far a decision model's probability may be from the package's golden (Q11).
const DECISION_TOLERANCE: f64 = 0.01;
/// The goldens a row without a GGUF is compared with, one per request fixture.
const GOLDENS: &str = "crates/ardana-core/tests/data/thx01";
/// The files a THX-01 pull records in its snapshot (R8.1).
const DECISION_FILES: [&str; 6] = [
    "config.json",
    "rl_agent_config.json",
    "encoder/config.json",
    "tokenizer/tokenizer.json",
    "tokenizer/tokenizer_config.json",
    "model.safetensors",
];
/// Options a decision model's choice question is refused above (Q10).
const DECISION_MAX_OPTIONS: usize = 24;
/// How often a `long_state` row repeats the ticket's state (18 tokens a time in Qwen2.5's tokenizer).
const LONG_STATE_REPEATS: usize = 300;
/// The tokens a long state must exceed.
const LONG_STATE_TOKENS: u64 = 4500;
/// How long the server may take over one request, its first load included.
const ANSWER_SECONDS: &str = "900";

/// Runs every row, a failed one included in the count but not ending the suite, then `cargo clean`.
pub fn candle(sandbox: &Sandbox, fixtures: &[CandleFixture]) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let mut failed = Vec::new();
    for fixture in fixtures {
        let started = Instant::now();
        match row(sandbox, &ardana, fixture) {
            Ok(()) => println!(
                "e2e candle {}: passed in {}",
                fixture.name,
                seconds(started.elapsed())
            ),
            Err(err) => {
                println!("e2e candle {}: failed: {err:#}", fixture.name);
                failed.push(fixture.name);
            }
        }
    }
    let cleaned = run(sandbox
        .command(cargo())
        .current_dir(sandbox.repo_root())
        .arg("clean"));
    ensure!(
        failed.is_empty(),
        "{} of {} candle rows failed: {}",
        failed.len(),
        fixtures.len(),
        failed.join(", ")
    );
    cleaned
}

/// One row: its checks, then, whatever they found, its registry entries and every file it added to the hub cache
/// removed.
fn row(sandbox: &Sandbox, ardana: &Path, fixture: &CandleFixture) -> Result<()> {
    let hub = sandbox.tmp().join("hf/hub");
    let before = paths(&hub)?;
    let cli = Cli::new(sandbox, ardana, fixture.name)?;
    let mut pulled = Vec::new();
    let checked = match &fixture.gguf {
        Some((gguf, file)) => check(&cli, fixture, gguf, file, &mut pulled),
        None => check_decision(&cli, fixture, &mut pulled),
    };
    let removed = cli
        .remove(&pulled)
        .and(remove_added(&hub, &before))
        .context("removing what the row pulled");
    match &removed {
        Ok(bytes) => println!(
            "e2e candle {}: removed its entries ({}) and {} from {}",
            fixture.name,
            pulled.join(", "),
            gigabytes(*bytes),
            hub.display()
        ),
        Err(err) => println!("e2e candle {}: {err:#}", fixture.name),
    }
    checked.and(removed.map(drop))
}

/// R3.1 to R3.4 for one row; `pulled` gets each entry once its pull recorded it.
fn check(
    cli: &Cli,
    fixture: &CandleFixture,
    gguf: &Pin,
    file: &str,
    pulled: &mut Vec<String>,
) -> Result<()> {
    let candle = fixture.name.to_string();
    let llama = format!("{}-gguf", fixture.name);
    let checkpoint = format!("hf.co/{}", fixture.checkpoint.repo);
    cli.pull(&[&checkpoint, "--layout", "chat", "--name", &candle])?;
    pulled.push(candle.clone());
    cli.pinned(&candle, "candle", &fixture.checkpoint)?;
    let reference = format!("hf.co/{}:{file}", gguf.repo);
    cli.pull(&[
        &reference,
        "--tokenizer",
        &checkpoint,
        "--layout",
        "chat",
        "--name",
        &llama,
    ])?;
    pulled.push(llama.clone());
    cli.pinned(&llama, "llama.cpp", gguf)?;

    let requests = cli.sandbox.repo_root().join("tests/fixtures/requests");
    let mut answered = Vec::new();
    for request in REQUESTS {
        let path = requests.join(request);
        let (got, want) = cli.compare(fixture.name, request, &path, &candle, &llama)?;
        answered.push((request, path, got, want));
    }
    let wanted: Vec<(&str, &Path, &Value)> = answered
        .iter()
        .map(|(request, path, _, want)| (*request, path.as_path(), want))
        .collect();
    served(cli, &candle, &wanted, |_, got, want| {
        same_answers(got, want, TOLERANCE).map(drop)
    })?
    .stop()?;
    if fixture.long_state {
        let (_, ticket, short, _) = &answered[0];
        long_state(cli, fixture.name, ticket, short, &candle, &llama)?;
    }
    Ok(())
}

/// R8.1, R8.3, R8.4, R8.5 and R9.1 for a decision-model row: the pull records runtime `candle` at the pin with the
/// six files in its snapshot, `tokenizer/tokenizer.json` as the tokenizer and the kind `decision`; `ardana run --json`
/// gives each fixture the golden's choices, every probability within [`DECISION_TOLERANCE`] and the golden's
/// `input_tokens`; a state past the window and a choice of 25 options are refused naming the window and the limit;
/// `--verbose` prints the model, the tokens and the timings, and `ardana list` sizes the entry by its safetensors;
/// one `ardana serve` on the row's home answers the fixtures as the goldens too, and `ardana ps` lists the model it
/// loaded for them.
fn check_decision(cli: &Cli, fixture: &CandleFixture, pulled: &mut Vec<String>) -> Result<()> {
    let name = fixture.name.to_string();
    let checkpoint = format!("hf.co/{}", fixture.checkpoint.repo);
    cli.pull(&[&checkpoint, "--name", &name])?;
    pulled.push(name.clone());
    let entry = cli.pinned(&name, "candle", &fixture.checkpoint)?;
    let weights = Path::new(entry["weights"].as_str().context("weights")?);
    for file in DECISION_FILES {
        ensure!(
            weights.join(file).is_file(),
            "the pull left {file} out of {}",
            weights.display()
        );
    }
    ensure!(
        Path::new(entry["tokenizer"].as_str().context("tokenizer")?)
            == weights.join("tokenizer/tokenizer.json"),
        "the tokenizer is {}, not the snapshot's tokenizer/tokenizer.json",
        entry["tokenizer"]
    );
    ensure!(
        entry["profile"]["decision"].is_object(),
        "the profile carries no decision format: {}",
        entry["profile"]
    );
    let shown = run_stdout(cli.command().args(["show", &name]))?;
    let kind = shown
        .lines()
        .find(|line| line.trim_start().starts_with("kind "))
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .with_context(|| format!("ardana show {name} prints no kind:\n{shown}"))?;
    ensure!(
        kind.contains("decision"),
        "ardana show {name} prints `{kind}`, not the kind decision"
    );
    println!(
        "e2e candle {name}: pulled at {} with the six files, {kind}",
        fixture.checkpoint.commit
    );

    let root = cli.sandbox.repo_root();
    let requests = root.join("tests/fixtures/requests");
    let goldens = root.join(GOLDENS);
    let mut answered = Vec::new();
    for request in REQUESTS {
        let path = requests.join(request);
        let want = read_json(&goldens.join(request))?;
        let (got, took) = cli.answer(&name, &path)?;
        let gap = same_as_golden(&got, &want).with_context(|| {
            format!("{request}: Candle answered {got}, the thx01 package {want}")
        })?;
        println!(
            "e2e candle {name} {request}: {} (thx01: {}), probabilities within {gap:.4}, {} tokens; Candle {}",
            summary(&got),
            summary(&want),
            got["usage"]["input_tokens"],
            seconds(took)
        );
        answered.push((request, path, want));
    }

    let ticket = read_json(&requests.join(REQUESTS[0]))?;
    let mut long = ticket.clone();
    let state = long["state"].as_str().context("the ticket's state")?;
    long["state"] = vec![state; LONG_STATE_REPEATS].join(" ").into();
    let window = entry["profile"]["decision"]["window"]
        .as_u64()
        .context("the window")?;
    let refused = cli.refused(&name, "long-state.json", &long)?;
    ensure!(
        refused.contains("request too large") && refused.contains(&format!("{window} tokens")),
        "a state past the window was refused with `{refused}`, not naming the {window}-token window"
    );
    let mut wide = ticket.clone();
    let options: Vec<String> = (0..=DECISION_MAX_OPTIONS)
        .map(|i| format!("team{i}"))
        .collect();
    wide["questions"]["department"]["criteria"] = options.into();
    let refused = cli.refused(&name, "wide-choice.json", &wide)?;
    ensure!(
        refused.contains("invalid request") && refused.contains(&DECISION_MAX_OPTIONS.to_string()),
        "a choice of {} options was refused with `{refused}`, not naming the {DECISION_MAX_OPTIONS}-option limit",
        DECISION_MAX_OPTIONS + 1
    );
    println!(
        "e2e candle {name}: a state past the {window}-token window and a choice of {} options refused",
        DECISION_MAX_OPTIONS + 1
    );

    let output = cli
        .command()
        .args(["run", &name, "--request"])
        .arg(requests.join(REQUESTS[0]))
        .args(["--json", "--verbose"])
        .output()
        .context("running ardana run --verbose")?;
    ensure!(
        output.status.success(),
        "ardana run --verbose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let verbose = String::from_utf8_lossy(&output.stderr);
    let golden = read_json(&goldens.join(REQUESTS[0]))?;
    for line in [
        format!("model: {}", golden["model"].as_str().unwrap_or_default()),
        format!("input tokens: {}", golden["usage"]["input_tokens"]),
        "load duration:".to_string(),
        "answer duration:".to_string(),
    ] {
        ensure!(
            verbose.lines().any(|l| l
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .starts_with(&line)),
            "ardana run --verbose printed no `{line}` line:
{verbose}"
        );
    }
    let listed = run_stdout(cli.command().arg("list"))?;
    let row = listed
        .lines()
        .find(|line| line.starts_with(&format!("{name} ")))
        .with_context(|| {
            format!(
                "ardana list has no row {name}:
{listed}"
            )
        })?;
    let cells: Vec<&str> = row.split_whitespace().collect();
    let bytes = std::fs::metadata(weights.join("model.safetensors"))?.len() as f64;
    let (size, unit) = (
        cells.get(1).and_then(|s| s.parse::<f64>().ok()),
        cells.get(2).copied(),
    );
    ensure!(
        unit == Some("MB") && size.is_some_and(|size| (size - bytes / 1e6).abs() <= 0.05),
        "ardana list sizes {name} as `{row}`, not by its {bytes}-byte model.safetensors"
    );
    println!(
        "e2e candle {name}: --verbose prints the model, tokens and timings; listed at {} {}",
        cells[1], cells[2]
    );

    let wanted: Vec<(&str, &Path, &Value)> = answered
        .iter()
        .map(|(request, path, want)| (*request, path.as_path(), want))
        .collect();
    let server = served(cli, &name, &wanted, |request, got, want| {
        let gap = same_as_golden(got, want)?;
        println!(
            "e2e candle {name} serve {request}: {} (thx01: {}), probabilities within {gap:.4}, {} tokens",
            summary(got),
            summary(want),
            got["usage"]["input_tokens"]
        );
        Ok(())
    })?;
    let port = server.url.rsplit(':').next().context("the server's port")?;
    let loaded = run_stdout(cli.command().args(["ps", "--port", port]))?;
    let row = loaded
        .lines()
        .find(|line| line.starts_with(&format!("{name} ")))
        .with_context(|| format!("ardana ps lists no {name} after its answers:\n{loaded}"))?;
    let row = row.split_whitespace().collect::<Vec<_>>().join(" ");
    ensure!(
        row.ends_with(&format!(" {checkpoint}")),
        "ardana ps lists {name} as `{row}`, not from {checkpoint}"
    );
    println!("e2e candle {name} serve: ardana ps lists `{row}`");
    server.stop()
}

/// Q11: `got` answers as the golden `want` does ([`same_answers`] within [`DECISION_TOLERANCE`]) and reports the
/// golden's `input_tokens`. Returns the widest gap between two probabilities.
fn same_as_golden(got: &Value, want: &Value) -> Result<f64> {
    let gap = same_answers(got, want, DECISION_TOLERANCE)?;
    let tokens = |response: &Value| response["usage"]["input_tokens"].as_u64();
    ensure!(
        tokens(got) == tokens(want),
        "{:?} input tokens, the thx01 package reports {:?}",
        tokens(got),
        tokens(want)
    );
    Ok(gap)
}

/// R3.3 and R9.1: one `ardana serve` on the row's home answers the fixtures in turn through Candle, each as `agrees`
/// holds it (request, answer, wanted) to the answer the row compares with (the GGUF's, or the golden), then the first
/// one again with the very answers it gave before: no prompt leaves anything to the next. Returns the server, still
/// running with the model loaded.
fn served(
    cli: &Cli,
    candle: &str,
    answered: &[(&str, &Path, &Value)],
    agrees: impl Fn(&str, &Value, &Value) -> Result<()>,
) -> Result<Server> {
    let server = Server::start_on(cli.sandbox, cli.ardana, &cli.home)?;
    let mut answers = Vec::new();
    for (request, path, want) in answered.iter().chain(answered.first()) {
        let mut body = read_json(path)?;
        body["model"] = candle.into();
        let output = cli
            .sandbox
            .command("curl")
            .args(["-fsS", "--max-time", ANSWER_SECONDS])
            .args(["-H", "content-type: application/json", "--data-binary"])
            .arg(body.to_string())
            .arg(format!("{}/v1/systemone", server.url))
            .output()
            .context("running curl")?;
        ensure!(
            output.status.success(),
            "the server did not answer {request}: {}\n{}",
            String::from_utf8_lossy(&output.stderr).trim(),
            server.log_text()
        );
        let got: Value = serde_json::from_slice(&output.stdout)
            .with_context(|| format!("parsing the server's answer to {request}"))?;
        agrees(request, &got, want).with_context(|| format!("{request} from the server: {got}"))?;
        answers.push(got["answers"].clone());
    }
    let (first, again) = (&answers[0], &answers[answers.len() - 1]);
    ensure!(
        first == again,
        "the server answered {} with {again} after the other fixtures, and with {first} before them",
        answered[0].0
    );
    println!(
        "e2e candle {candle} serve: {}, then {} again with the same probabilities",
        REQUESTS.join(", "),
        answered[0].0
    );
    Ok(server)
}

/// R3.4: the ticket's questions over its state repeated, through both runtimes. `short` is the answer to the ticket
/// as it is, which tells how many tokens the repeats added.
fn long_state(
    cli: &Cli,
    row: &str,
    ticket: &Path,
    short: &Value,
    candle: &str,
    llama: &str,
) -> Result<()> {
    let mut request = read_json(ticket)?;
    let state = request["state"].as_str().context("the ticket's state")?;
    request["state"] = vec![state; LONG_STATE_REPEATS].join(" ").into();
    let path = cli.home.with_file_name("long-state.json");
    std::fs::write(&path, request.to_string())
        .with_context(|| format!("writing {}", path.display()))?;
    let (got, _) = cli.compare(row, "long state", &path, candle, llama)?;
    let tokens = |response: &Value| response["usage"]["input_tokens"].as_u64();
    let added = tokens(&got)
        .zip(tokens(short))
        .map(|(long, short)| long.saturating_sub(short))
        .context("input_tokens")?;
    ensure!(
        added > LONG_STATE_TOKENS,
        "{LONG_STATE_REPEATS} repeats of the ticket's state add {added} tokens, not more than {LONG_STATE_TOKENS}"
    );
    println!("e2e candle {row} long state: {added} tokens more than the ticket's");
    Ok(())
}

/// The release `ardana` on a row's own `ARDANA_HOME`, `tmp/e2e/candle/<row>/home`, over the sandbox's hub cache.
struct Cli<'a> {
    sandbox: &'a Sandbox,
    ardana: &'a Path,
    home: PathBuf,
}

impl<'a> Cli<'a> {
    fn new(sandbox: &'a Sandbox, ardana: &'a Path, row: &str) -> Result<Cli<'a>> {
        let dir = sandbox.tmp().join("e2e/candle").join(row);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("emptying {}", dir.display()))?;
        }
        let home = dir.join("home");
        std::fs::create_dir_all(&home)?;
        Ok(Cli {
            sandbox,
            ardana,
            home,
        })
    }

    fn command(&self) -> Command {
        let mut cmd = self.sandbox.command(self.ardana);
        cmd.env("ARDANA_HOME", &self.home)
            .env("ARDANA_NO_UPDATE_CHECK", "1");
        cmd
    }

    /// `ardana pull <args>`, from the Hub.
    fn pull(&self, args: &[&str]) -> Result<()> {
        run(self.command().arg("pull").args(args))
    }

    /// Checks that the entry `name` runs through `runtime` and that its weights are `pin`'s commit: a pull reads a
    /// repository at `main`, so one that moved since the row was written fails here. Returns the entry as
    /// `ardana show --json` prints it.
    fn pinned(&self, name: &str, runtime: &str, pin: &Pin) -> Result<Value> {
        let entry: Value =
            serde_json::from_str(&run_stdout(self.command().args(["show", name, "--json"]))?)
                .with_context(|| format!("parsing ardana show {name} --json"))?;
        ensure!(
            entry["runtime"] == runtime,
            "{name} is recorded with the runtime {}, not {runtime}",
            entry["runtime"]
        );
        // A checkpoint's weights are its snapshot directory, a GGUF's a file in one.
        let weights = Path::new(entry["weights"].as_str().context("weights")?);
        let snapshot = if weights.is_dir() {
            weights
        } else {
            weights.parent().context("a weights file in a snapshot")?
        };
        ensure!(
            snapshot
                .file_name()
                .is_some_and(|commit| commit == pin.commit),
            "hf.co/{} is now at {}, not at the commit {} the row pins",
            pin.repo,
            snapshot.display(),
            pin.commit
        );
        Ok(entry)
    }

    /// `ardana run <name> --request <request>` of `body`, written to `<file>` beside the home, which must fail:
    /// returns its stderr.
    fn refused(&self, name: &str, file: &str, body: &Value) -> Result<String> {
        let path = self.home.with_file_name(file);
        std::fs::write(&path, body.to_string())
            .with_context(|| format!("writing {}", path.display()))?;
        let output = self
            .command()
            .args(["run", name, "--request"])
            .arg(&path)
            .arg("--json")
            .output()
            .context("running ardana run")?;
        ensure!(
            !output.status.success(),
            "{name} answered {} instead of refusing it: {}",
            path.display(),
            String::from_utf8_lossy(&output.stdout)
        );
        Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }

    /// `ardana run <name> --request <request> --json`: the response, and how long the command took.
    fn answer(&self, name: &str, request: &Path) -> Result<(Value, Duration)> {
        let started = Instant::now();
        let stdout = run_stdout(
            self.command()
                .args(["run", name, "--request"])
                .arg(request)
                .arg("--json"),
        )?;
        let response = serde_json::from_str(&stdout)
            .with_context(|| format!("parsing {name}'s answer to {}", request.display()))?;
        Ok((response, started.elapsed()))
    }

    /// R3.2: `request` through both entries, Candle's answers checked against the GGUF's; returns both responses,
    /// Candle's first.
    fn compare(
        &self,
        row: &str,
        label: &str,
        request: &Path,
        candle: &str,
        llama: &str,
    ) -> Result<(Value, Value)> {
        let (want, llama_took) = self.answer(llama, request)?;
        let (got, candle_took) = self.answer(candle, request)?;
        let gap = same_answers(&got, &want, TOLERANCE)
            .with_context(|| format!("{label}: Candle answered {got}, llama.cpp {want}"))?;
        println!(
            "e2e candle {row} {label}: {} (llama.cpp: {}), probabilities within {gap:.4}; Candle {}, llama.cpp {}",
            summary(&got),
            summary(&want),
            seconds(candle_took),
            seconds(llama_took)
        );
        Ok((got, want))
    }

    /// `ardana rm` of the entries a row pulled.
    fn remove(&self, names: &[String]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        run_stdout(self.command().arg("rm").args(names)).map(drop)
    }
}

/// Checks that `got` answers every question of `want` alike: the same choice, the same side of 0.5 for a noul, and
/// every probability (a noul's, each option's, each score level's) within `tolerance`. Returns the widest gap
/// between two probabilities.
fn same_answers(got: &Value, want: &Value, tolerance: f64) -> Result<f64> {
    let answers = want["answers"].as_object().context("answers")?;
    ensure!(!answers.is_empty(), "no answers in {want}");
    let mut widest = 0.0_f64;
    for (key, want) in answers {
        let got = &got["answers"][key];
        let pairs: Vec<(&Value, &Value)> = match want["type"].as_str() {
            Some("noul") => {
                let (g, w) = (&got["noul"], &want["noul"]);
                ensure!(
                    g.as_f64().map(|p| p > 0.5) == w.as_f64().map(|p| p > 0.5),
                    "{key}: noul {g}, not on the side of {w}"
                );
                vec![(g, w)]
            }
            Some(kind @ ("choice" | "score")) => {
                ensure!(
                    kind == "score" || got["choice"] == want["choice"],
                    "{key}: chose {}, not {}",
                    got["choice"],
                    want["choice"]
                );
                let probabilities = want["probabilities"].as_object().context("probabilities")?;
                probabilities
                    .iter()
                    .map(|(option, p)| (&got["probabilities"][option], p))
                    .collect()
            }
            other => bail!("{key}: an answer of type {other:?}"),
        };
        for (g, w) in pairs {
            let gap = g
                .as_f64()
                .zip(w.as_f64())
                .map(|(g, w)| (g - w).abs())
                .with_context(|| format!("{key}: {g} and {w} are not two probabilities"))?;
            ensure!(
                gap <= tolerance,
                "{key}: {g} and {w} differ by more than {tolerance}"
            );
            widest = widest.max(gap);
        }
    }
    Ok(widest)
}

/// A response's answers on one line: each question with its choice, its score or its probability of yes.
fn summary(response: &Value) -> String {
    let answers = response["answers"].as_object();
    let answers = answers.into_iter().flatten().map(|(key, answer)| {
        match &answer[answer["type"].as_str().unwrap_or_default()] {
            Value::String(choice) => format!("{key} {choice}"),
            figure => format!("{key} {figure}"),
        }
    });
    answers.collect::<Vec<_>>().join(", ")
}

fn seconds(took: Duration) -> String {
    format!("{:.1} s", took.as_secs_f64())
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

/// Every path under `dir`, links not followed; none when `dir` is missing.
fn paths(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    fn walk(dir: &Path, into: &mut BTreeSet<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                walk(&entry.path(), into)?;
            }
            into.insert(entry.path());
        }
        Ok(())
    }
    let mut paths = BTreeSet::new();
    if dir.is_dir() {
        walk(dir, &mut paths)?;
    }
    Ok(paths)
}

/// R3.5: removes every path under `dir` that is not in `before`, so a repository the cache held in part keeps the
/// files it had and loses the ones added since; returns the bytes of the files removed.
fn remove_added(dir: &Path, before: &BTreeSet<PathBuf>) -> Result<u64> {
    let mut bytes = 0;
    if !dir.is_dir() {
        return Ok(bytes);
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let added = !before.contains(&path);
        if entry.file_type()?.is_dir() {
            bytes += remove_added(&path, before)?;
            if added {
                std::fs::remove_dir(&path)
                    .with_context(|| format!("removing {}", path.display()))?;
            }
        } else if added {
            bytes += entry.metadata()?.len();
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn response(billing: f64, refund: f64, level: f64) -> Value {
        json!({"answers": {
            "department": {
                "type": "choice",
                "choice": if billing > 0.5 { "billing" } else { "sales" },
                "probabilities": {"billing": billing, "sales": 1.0 - billing},
            },
            "refund": {"type": "noul", "noul": refund},
            "charges": {"type": "score", "score": 1.0, "probabilities": {"0": 1.0 - level, "1": level}},
        }})
    }

    /// R3.2: answers within the tolerance pass with their widest gap; another choice, another noul side, a
    /// probability of any question type past the tolerance and a missing answer do not.
    #[test]
    fn same_answers_within_the_tolerance() {
        let want = response(0.9, 0.8, 0.7);
        let gap = same_answers(&response(0.85, 0.82, 0.61), &want, TOLERANCE).unwrap();
        assert!((gap - 0.09).abs() < 1e-9, "{gap}");
        let err = same_answers(&response(0.85, 0.82, 0.61), &want, DECISION_TOLERANCE).unwrap_err();
        assert!(
            err.to_string().contains("differ by more than 0.01"),
            "{err:#}"
        );
        let gap = same_answers(&response(0.895, 0.8, 0.7), &want, DECISION_TOLERANCE).unwrap();
        assert!((gap - 0.005).abs() < 1e-9, "{gap}");
        for (other, problem) in [
            (response(0.45, 0.8, 0.7), "chose"),
            (response(0.9, 0.45, 0.7), "not on the side"),
            (response(0.75, 0.8, 0.7), "differ by more"),
            (response(0.9, 0.65, 0.7), "differ by more"),
            (response(0.9, 0.8, 0.55), "differ by more"),
            (json!({"answers": {}}), "chose null"),
        ] {
            let err = same_answers(&other, &want, TOLERANCE).unwrap_err();
            assert!(err.to_string().contains(problem), "{other}: {err:#}");
        }
        assert!(same_answers(&want, &json!({"answers": {}}), TOLERANCE).is_err());
        assert_eq!(
            summary(&want),
            "department billing, refund 0.8, charges 1.0"
        );
    }

    /// R3.5: what a row added goes, in a directory that is new and in one that was there, links included; what was
    /// there stays.
    #[test]
    fn remove_added_keeps_what_was_there() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("candle-remove-added-{}", std::process::id()));
        let old = dir.join("models--a--old");
        std::fs::create_dir_all(old.join("blobs"))?;
        std::fs::create_dir_all(old.join("snapshots/c1"))?;
        std::fs::write(old.join("blobs/tokenizer"), "12345")?;
        std::os::unix::fs::symlink(
            "../../blobs/tokenizer",
            old.join("snapshots/c1/tokenizer.json"),
        )?;
        let before = paths(&dir)?;
        assert_eq!(remove_added(&dir, &before)?, 0);

        std::fs::write(old.join("blobs/weights"), "1234567")?;
        std::os::unix::fs::symlink(
            "../../blobs/weights",
            old.join("snapshots/c1/model.safetensors"),
        )?;
        let new = dir.join("models--a--new/snapshots/c2");
        std::fs::create_dir_all(&new)?;
        std::fs::write(new.join("model.gguf"), "123")?;
        std::fs::create_dir_all(dir.join(".locks/models--a--new"))?;
        let link = "../../blobs/weights".len() as u64;
        assert_eq!(remove_added(&dir, &before)?, 7 + link + 3);
        assert_eq!(paths(&dir)?, before);
        assert_eq!(
            std::fs::read_to_string(old.join("snapshots/c1/tokenizer.json"))?,
            "12345"
        );
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
