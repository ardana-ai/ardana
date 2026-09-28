//! Shared helpers of the `ardana` end-to-end tests: the Hub files `cargo xtask fetch` put into `tmp/hf`, the request
//! fixtures, `ardana` in its own home, and the R2.7 and R3.5 answer checks.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// A file of the pinned snapshot of `repo` (`org/name`) in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
pub fn hf_file(repo: &str, name: &str) -> Result<PathBuf> {
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
    let path = dir.join("snapshots").join(rev.trim()).join(name);
    ensure!(
        path.is_file(),
        "{} is missing; run `cargo xtask fetch`",
        path.display()
    );
    Ok(path)
}

/// A request fixture in `tests/fixtures/requests`.
pub fn request(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/requests")
        .join(name)
}

/// Runs `ardana run <args>`, prints its output and parses the response.
pub fn ardana_run<I, S>(args: I) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ardana"));
    cmd.arg("run").args(args);
    parse_run(&mut cmd)
}

/// Runs `cmd`, an `ardana run`, prints its output and parses the response.
fn parse_run(cmd: &mut Command) -> Result<Value> {
    let output = cmd.output().context("running ardana run")?;
    ensure!(
        output.status.success(),
        "{cmd:?} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    println!("{cmd:?}:\n{stdout}");
    serde_json::from_str(&stdout).context("ardana run printed JSON")
}

/// `$ARDANA_TMP/<name>`, emptied: each test's own scratch directory.
pub fn scratch(name: &str) -> Result<PathBuf> {
    let tmp = PathBuf::from(std::env::var_os("ARDANA_TMP").context("ARDANA_TMP is not set")?);
    let dir = tmp.join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("emptying {}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// `ardana` with its own `ARDANA_HOME`, resolving Hub files offline from `tmp/hf` (`HF_HUB_OFFLINE=1`).
pub struct Ardana {
    pub home: PathBuf,
}

impl Ardana {
    /// An empty home in `$ARDANA_TMP/<test>/home`.
    pub fn new(test: &str) -> Result<Ardana> {
        Ok(Ardana {
            home: scratch(test)?.join("home"),
        })
    }

    /// `ardana <args>` in this home.
    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ardana"));
        cmd.args(args)
            .env("ARDANA_HOME", &self.home)
            .env("HF_HUB_OFFLINE", "1");
        cmd
    }

    /// Runs `ardana <args>` and returns its output, printing it.
    pub fn output<I, S>(&self, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.command(args);
        run(&mut cmd)
    }

    /// Runs `ardana <args>`, which must succeed, and returns its stdout.
    pub fn ok<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.command(args);
        let output = run(&mut cmd)?;
        ensure!(
            output.status.success(),
            "{cmd:?} failed ({})",
            output.status
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    /// `ardana run <name> --request <request> <extra>`, parsed.
    pub fn run_named(&self, name: &str, request: &Path, extra: &[&str]) -> Result<Value> {
        let mut cmd = self.command(["run", name, "--request"]);
        cmd.arg(request).args(extra);
        parse_run(&mut cmd)
    }

    /// `models.toml` in this home, parsed.
    pub fn models_toml(&self) -> Result<toml::Table> {
        let path = self.home.join("models.toml");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Ok(toml::from_str(&text)?)
    }

    /// The `[[model]]` table named `name` in `models.toml`.
    pub fn entry(&self, name: &str) -> Result<toml::Table> {
        let file = self.models_toml()?;
        let models = file
            .get("model")
            .and_then(toml::Value::as_array)
            .context("models.toml has no [[model]]")?;
        models
            .iter()
            .filter_map(toml::Value::as_table)
            .find(|m| m.get("name").and_then(toml::Value::as_str) == Some(name))
            .cloned()
            .with_context(|| format!("models.toml has no model {name}"))
    }
}

/// Runs `cmd` and prints what it wrote.
fn run(cmd: &mut Command) -> Result<Output> {
    let output = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    println!(
        "{cmd:?} ({}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

/// R2.7 on the ticket fixture: `billing`, refund >= 0.9, probabilities summing to 1 +- 0.001; returns the argmaxes.
pub fn check_ticket(resp: &Value) -> Result<(String, bool)> {
    assert_eq!(resp["model"], "decider-2b-v11");
    let department = &resp["answers"]["department"];
    assert_eq!(department["type"], "choice");
    assert_eq!(department["choice"], "billing", "{department}");
    let probabilities = department["probabilities"]
        .as_object()
        .context("probabilities")?;
    assert_eq!(
        probabilities.keys().collect::<Vec<_>>(),
        ["billing", "technical", "sales"]
    );
    let sum = probability_sum(department)?;
    assert!((sum - 1.0).abs() <= 0.001, "probabilities sum to {sum}");
    let refund = resp["answers"]["refund"]["noul"]
        .as_f64()
        .context("refund noul")?;
    assert!(refund >= 0.9, "refund {refund}");
    assert_eq!(resp["usage"]["output_tokens"], 0);
    assert!(
        resp["usage"]["input_tokens"]
            .as_u64()
            .is_some_and(|n| n > 0)
    );
    Ok((
        department["choice"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        refund > 0.5,
    ))
}

/// R3.5 on the sentiment fixture: `positive`, happy > 0.5, probabilities summing to 1 +- 0.001.
pub fn check_sentiment(resp: &Value) -> Result<()> {
    let sentiment = &resp["answers"]["sentiment"];
    assert_eq!(sentiment["choice"], "positive", "{sentiment}");
    let happy = resp["answers"]["happy"]["noul"]
        .as_f64()
        .context("happy noul")?;
    assert!(happy > 0.5, "happy {happy}");
    let sum = probability_sum(sentiment)?;
    assert!((sum - 1.0).abs() <= 0.001, "probabilities sum to {sum}");
    Ok(())
}

/// The sum of a choice answer's probabilities.
pub fn probability_sum(answer: &Value) -> Result<f64> {
    let probabilities = answer["probabilities"]
        .as_object()
        .context("probabilities")?;
    Ok(probabilities.values().filter_map(Value::as_f64).sum())
}
