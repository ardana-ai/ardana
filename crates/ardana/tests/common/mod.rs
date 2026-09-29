//! Shared helpers of the `ardana` end-to-end tests: the Hub files `cargo xtask fetch` put into `tmp/hf`, the request
//! fixtures, `ardana` in its own home, and the R2.7 and R3.5 answer checks.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
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

/// Runs `ardana run <args> --json`, prints its output and parses the response.
pub fn ardana_run<I, S>(args: I) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ardana"));
    cmd.arg("run").args(args).arg("--json");
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

    /// `ardana run <name> --request <request> <extra> --json`, parsed.
    pub fn run_named(&self, name: &str, request: &Path, extra: &[&str]) -> Result<Value> {
        let mut cmd = self.command(["run", name, "--request"]);
        cmd.arg(request).args(extra).arg("--json");
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

/// `ardana serve` on a free port of 127.0.0.1 in an [`Ardana`] home, its stderr in `serve.log` beside the home.
/// Dropping it kills the server.
pub struct Served {
    child: Child,
    pub port: u16,
    pub log: PathBuf,
}

impl Served {
    /// Starts `ardana serve --port <free port> <extra>` with `env` and waits until `/health` answers.
    pub fn start(ardana: &Ardana, extra: &[&str], env: &[(&str, &str)]) -> Result<Served> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let log = ardana
            .home
            .parent()
            .context("the home has a parent")?
            .join("serve.log");
        let mut cmd = ardana.command(["serve", "--port", &port.to_string()]);
        cmd.args(extra)
            .envs(env.iter().copied())
            .stderr(std::fs::File::create(&log)?)
            .stdout(Stdio::null());
        let served = Served {
            child: cmd.spawn()?,
            port,
            log,
        };
        let started = Instant::now();
        while served.get("/health", "").is_err() {
            if started.elapsed() > Duration::from_secs(10) {
                bail!(
                    "ardana serve did not answer on port {port}:\n{}",
                    served.log_text()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(served)
    }

    /// What the server wrote to stderr so far.
    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// `GET path` with extra `headers` (each ending in `\r\n`); the status and body.
    pub fn get(&self, path: &str, headers: &str) -> Result<(u16, String)> {
        http(self.port, &format!("GET {path}"), headers, "")
    }

    /// `POST /v1/systemone` with a JSON body; the status and the parsed body.
    pub fn decide(&self, body: &Value) -> Result<(u16, Value)> {
        let (status, text) = http(
            self.port,
            "POST /v1/systemone",
            "Content-Type: application/json\r\n",
            &body.to_string(),
        )?;
        let body = serde_json::from_str(&text).with_context(|| format!("{status}: {text:?}"))?;
        Ok((status, body))
    }

    /// `GET /v1/models`, parsed.
    pub fn models(&self) -> Result<Value> {
        let (status, text) = self.get("/v1/models", "")?;
        ensure!(status == 200, "GET /v1/models answered {status}: {text}");
        Ok(serde_json::from_str(&text)?)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        println!("{}:\n{}", self.log.display(), self.log_text());
    }
}

/// One HTTP/1.1 request (`<METHOD> <path>`) on 127.0.0.1:`port`, the connection closed after it; the status and body.
pub fn http(port: u16, request_line: &str, headers: &str, body: &str) -> Result<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    write!(
        stream,
        "{request_line} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .with_context(|| format!("no status in {response:?}"))?;
    let body = response.split("\r\n\r\n").nth(1).unwrap_or_default();
    Ok((status, body.to_string()))
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
