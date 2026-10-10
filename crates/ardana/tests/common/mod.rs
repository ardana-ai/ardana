//! Shared helpers of the `ardana` end-to-end tests: the Hub files `cargo xtask fetch` put into `tmp/hf`, the request
//! fixtures, the library snapshot and a loopback server publishing it, `ardana` in its own home, and the R2.7 and R3.5
//! answer checks.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

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

/// The pinned snapshot directory of `repo` (`org/name`) in `$HF_HOME/hub`, as `cargo xtask fetch` wrote it.
pub fn hf_snapshot(repo: &str) -> Result<PathBuf> {
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
    Ok(dir.join("snapshots").join(rev.trim()))
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

/// The library snapshot (C5): the document cargo names as `ARDANA_LIBRARY` for every test.
pub fn snapshot_path() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("ARDANA_LIBRARY").context("cargo sets ARDANA_LIBRARY")?,
    ))
}

/// The snapshot's bytes.
pub fn snapshot_bytes() -> Result<Vec<u8>> {
    let path = snapshot_path()?;
    std::fs::read(&path).with_context(|| format!("reading {}", path.display()))
}

/// The snapshot, parsed.
pub fn snapshot() -> Result<Value> {
    Ok(serde_json::from_slice(&snapshot_bytes()?)?)
}

/// The snapshot's sizes under their canonical names `<family>:<size>`, in its order.
pub fn snapshot_names() -> Result<Vec<String>> {
    let mut names = Vec::new();
    for family in snapshot()?["models"].as_array().context("models")? {
        for size in family["sizes"].as_array().context("sizes")? {
            names.push(format!(
                "{}:{}",
                family["name"].as_str().context("a family name")?,
                size["size"].as_str().context("a size")?
            ));
        }
    }
    Ok(names)
}

/// The snapshot with one more size of the decider family, `size`: the size `2b` under that tag over the repository
/// `hf.co/test/decider-<size>-GGUF`, without a browser variant, listed as `decider:<size>` after the family's others.
pub fn snapshot_with(size: &str) -> Result<Vec<u8>> {
    let mut document = snapshot()?;
    let sizes = document["models"]
        .as_array_mut()
        .and_then(|models| models.iter_mut().find(|m| m["name"] == "decider"))
        .and_then(|decider| decider["sizes"].as_array_mut())
        .context("the decider family's sizes")?;
    let mut added = sizes
        .iter()
        .find(|s| s["size"] == "2b")
        .context("decider:2b")?
        .clone();
    added["size"] = json!(size);
    added["gguf"]["repo"] = json!(format!("hf.co/test/decider-{size}-GGUF"));
    added.as_object_mut().context("a size")?.remove("browser");
    sizes.push(added);
    Ok(serde_json::to_vec_pretty(&document)?)
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

/// Polls `check` every 50 ms until it holds, for up to `timeout`.
pub fn eventually(
    what: &str,
    timeout: Duration,
    mut check: impl FnMut() -> Result<bool>,
) -> Result<()> {
    let started = Instant::now();
    loop {
        if check()? {
            return Ok(());
        }
        ensure!(
            started.elapsed() < timeout,
            "timed out after {timeout:?} waiting until {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A request a [`LibraryServer`] answered: its path and the headers a library GET is checked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub path: String,
    pub if_none_match: Option<String>,
    pub user_agent: Option<String>,
}

/// A loopback server publishing a library document as the landing does (C2): the index at `/models.json` with an
/// `ETag` (a 304 when `If-None-Match` carries it), each family's manifest at `/models/<family>.json`, 404 elsewhere. The
/// document can be replaced while it runs ([`LibraryServer::publish`], a new `ETag`), and every request is recorded.
pub struct LibraryServer {
    /// `http://127.0.0.1:<port>/models.json`, what `ARDANA_LIBRARY` names.
    pub url: String,
    published: Arc<Mutex<(Vec<u8>, u64)>>,
    requests: Arc<Mutex<Vec<Seen>>>,
}

impl LibraryServer {
    pub fn start(document: &[u8]) -> Result<LibraryServer> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/models.json", listener.local_addr()?);
        let published = Arc::new(Mutex::new((document.to_vec(), 1)));
        let requests: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let (current, seen) = (published.clone(), requests.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let _ = answer(stream, &current, &seen);
            }
        });
        Ok(LibraryServer {
            url,
            published,
            requests,
        })
    }

    /// Publishes `document` in place of the one served so far, under a new `ETag`.
    pub fn publish(&self, document: &[u8]) {
        let mut published = self.published.lock().expect("the published document");
        *published = (document.to_vec(), published.1 + 1);
    }

    /// The `ETag` of the document served now, as sent.
    pub fn etag(&self) -> String {
        etag(self.published.lock().expect("the published document").1)
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.requests.lock().expect("the request log").clone()
    }

    /// The index GETs seen so far, in order.
    pub fn index_requests(&self) -> Vec<Seen> {
        self.requests()
            .into_iter()
            .filter(|seen| seen.path == "/models.json")
            .collect()
    }
}

fn etag(version: u64) -> String {
    format!("\"v{version}\"")
}

/// Answers the one request on `stream` from the document in `published`, recording it in `seen`.
fn answer(
    mut stream: TcpStream,
    published: &Mutex<(Vec<u8>, u64)>,
    seen: &Mutex<Vec<Seen>>,
) -> Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte)? == 1 {
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default()
        .to_string();
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (given, value) = line.split_once(':')?;
            given
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };
    let request = Seen {
        path: path.clone(),
        if_none_match: header("if-none-match"),
        user_agent: header("user-agent"),
    };
    seen.lock().expect("the request log").push(request.clone());
    let (document, version) = published.lock().expect("the published document").clone();
    let tag = etag(version);
    let (status, headers, body) = if path == "/models.json" {
        if request.if_none_match.as_deref() == Some(tag.as_str()) {
            ("304 Not Modified", format!("ETag: {tag}\r\n"), Vec::new())
        } else {
            ("200 OK", format!("ETag: {tag}\r\n"), document)
        }
    } else {
        let manifest = path
            .strip_prefix("/models/")
            .and_then(|file| file.strip_suffix(".json"))
            .and_then(|family| {
                let index: Value = serde_json::from_slice(&document).ok()?;
                let model = index["models"]
                    .as_array()?
                    .iter()
                    .find(|m| m["name"] == family)?
                    .clone();
                serde_json::to_vec(&json!({"schema": 1, "model": model})).ok()
            });
        match manifest {
            Some(body) => ("200 OK", String::new(), body),
            None => ("404 Not Found", String::new(), b"not found".to_vec()),
        }
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}

/// A loopback stand-in for the Hugging Face Hub serving one repository at the commit `cargo xtask fetch` pinned in
/// `tmp/hf`: `GET /api/models/<org>/<repo>` names that commit and lists `listing` as the repository's files, and
/// `HEAD` or `GET /<org>/<repo>/resolve/<commit>/<file>` answers a file the pinned snapshot holds with its blob name as
/// the `ETag` (a 404 for any other file). Every request is recorded as `<METHOD> <path>`.
pub struct HubStandIn {
    /// What `HF_ENDPOINT` names.
    pub endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl HubStandIn {
    pub fn start(repo: &str, listing: &[&str]) -> Result<HubStandIn> {
        let snapshot = hf_snapshot(repo)?;
        let commit = snapshot
            .file_name()
            .context("a snapshot directory")?
            .to_string_lossy()
            .into_owned();
        let info = serde_json::to_vec(&json!({
            "id": repo,
            "sha": commit,
            "siblings": listing.iter().map(|f| json!({"rfilename": f})).collect::<Vec<_>>(),
        }))?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = requests.clone();
        let (info_path, resolve) = (
            format!("/api/models/{repo}"),
            format!("/{repo}/resolve/{commit}/"),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let _ = answer_hub(
                    stream, &seen, &info_path, &info, &resolve, &snapshot, &commit,
                );
            }
        });
        Ok(HubStandIn { endpoint, requests })
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the request log").clone()
    }

    /// The files asked for by name, each once (a `HEAD` and a `GET` of a file are one), in order.
    pub fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = Vec::new();
        for request in self.requests() {
            let file = request
                .split_once("/resolve/")
                .and_then(|(_, rest)| rest.split_once('/'))
                .map(|(_, file)| file.to_string());
            if let Some(file) = file.filter(|file| !files.contains(file)) {
                files.push(file);
            }
        }
        files
    }

    /// A hub cache in `dir` holding the blobs of the pinned snapshot of `repo` (links to those in `tmp/hf`, so no
    /// weights are copied) and nothing else: a pull through it still asks the Hub for every file by name, and takes
    /// the blob the answer's `ETag` names without downloading it.
    pub fn cache_with_blobs(dir: &Path, repo: &str) -> Result<()> {
        let blobs = dir
            .join(format!("models--{}", repo.replace('/', "--")))
            .join("blobs");
        std::fs::create_dir_all(&blobs)?;
        for entry in std::fs::read_dir(hf_snapshot(repo)?)? {
            let blob = std::fs::canonicalize(entry?.path())?;
            let name = blob.file_name().context("a blob name")?;
            std::os::unix::fs::symlink(&blob, blobs.join(name))?;
        }
        Ok(())
    }
}

/// Answers the one request on `stream` as [`HubStandIn`] describes, recording it in `seen`.
fn answer_hub(
    mut stream: TcpStream,
    seen: &Mutex<Vec<String>>,
    info_path: &str,
    info: &[u8],
    resolve: &str,
    snapshot: &Path,
    commit: &str,
) -> Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte)? == 1 {
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut line = head.lines().next().unwrap_or_default().split(' ');
    let (method, target) = (
        line.next().unwrap_or_default(),
        line.next().unwrap_or_default(),
    );
    let path = target.split('?').next().unwrap_or_default();
    seen.lock()
        .expect("the request log")
        .push(format!("{method} {path}"));
    let file = path.strip_prefix(resolve).map(|file| snapshot.join(file));
    if path == info_path {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            info.len()
        )?;
        stream.write_all(info)?;
    } else if let Some(file) = file.filter(|file| file.is_file()) {
        let blob = std::fs::canonicalize(&file)?;
        let name = blob.file_name().context("a blob name")?.to_string_lossy();
        let size = std::fs::metadata(&blob)?.len();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nETag: \"{name}\"\r\nX-Repo-Commit: {commit}\r\nConnection: close\r\n\r\n"
        )?;
        if method == "GET" {
            std::io::copy(&mut std::fs::File::open(&blob)?, &mut stream)?;
        }
    } else {
        write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nX-Error-Code: EntryNotFound\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )?;
    }
    Ok(())
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
