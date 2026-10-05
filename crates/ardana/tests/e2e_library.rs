//! The model library end to end, offline from the files `cargo xtask fetch` put into `tmp/hf`: `ardana pull <name>`
//! recording a size under its canonical name with the file its quant names (Q14), `ardana run <name>` and `ardana
//! serve` pulling a library model on first use (the default one for `jev-latest` and requests without a model), `serve`
//! picking up a model `ardana pull` adds while it runs, and a pull online, against a stand-in of Hugging Face serving
//! `tmp/hf`, asking for nothing but the files and commits the library pins.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use common::{Ardana, LibraryServer, Served};
use serde_json::{Value, json};

/// `HF_HUB_OFFLINE=1` (set by [`Ardana`]) with an unreachable endpoint: nothing may call the Hub.
const NO_HUB: (&str, &str) = ("HF_ENDPOINT", "http://127.0.0.1:9");

fn request(name: &str) -> Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        common::request(name),
    )?)?)
}

/// The size `name` names in the snapshot, as JSON: its family and its size.
fn snapshot_size(name: &str) -> Result<(Value, Value)> {
    let (family, size) = name.split_once(':').context("a canonical name")?;
    let document = common::snapshot()?;
    let family = document["models"]
        .as_array()
        .and_then(|models| models.iter().find(|m| m["name"] == family))
        .with_context(|| format!("the library has {family}"))?
        .clone();
    let size = family["sizes"]
        .as_array()
        .and_then(|sizes| sizes.iter().find(|s| s["size"] == size))
        .with_context(|| format!("the library has {name}"))?
        .clone();
    Ok((family, size))
}

/// R2.4: a library pull records the size under its canonical name with the file its quant names at `gguf.commit`
/// (Q14), its tokenizer read at the tokenizer's commit and its layout the family's.
#[test]
#[ignore = "e2e: decider-2b Q4_K_M, Qwen3.5-0.8B, SmolLM3-3B and gemma-4-E2B GGUFs and tokenizers in tmp/hf (cargo xtask fetch)"]
fn pull_library_names() -> Result<()> {
    let ardana = Ardana::new("e2e-library-pull")?;
    let pull = |name: &str| {
        let mut cmd = ardana.command(["pull", name]);
        cmd.env(NO_HUB.0, NO_HUB.1);
        cmd.output()
    };

    let out = pull("decider")?;
    assert!(out.status.success(), "{out:?}");
    let decider = ardana.entry("decider:2b")?;
    assert_eq!(
        decider["source"].as_str(),
        Some("hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf")
    );
    assert_eq!(
        decider["weights"].as_str().map(PathBuf::from),
        Some(common::hf_file(
            "Mapika/decider-2b-GGUF",
            "decider-2b-v11-Q4_K_M.gguf"
        )?)
    );
    let profile = decider["profile"].as_table().context("profile")?;
    assert_eq!(
        profile["name"].as_str(),
        Some("decider-2b-v11"),
        "decider_config.json"
    );
    assert_eq!(profile["layout"]["kind"].as_str(), Some("plain"));

    // Another quant is its own file of the same repository; offline, only the fetched Q4_K_M is there.
    let out = pull("decider:2b-q8_0")?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(stderr.contains("decider-2b-v11-Q8_0.gguf"), "{stderr}");

    for (name, entry, gguf, tokenizer) in [
        (
            "qwen3.5",
            "qwen3.5:0.8b",
            ("ggml-org/Qwen3.5-0.8B-GGUF", "Qwen3.5-0.8B-Q4_0.gguf"),
            "Qwen/Qwen3.5-0.8B",
        ),
        (
            "smollm3:3b",
            "smollm3:3b",
            ("ggml-org/SmolLM3-3B-GGUF", "SmolLM3-Q4_K_M.gguf"),
            "HuggingFaceTB/SmolLM3-3B",
        ),
        (
            "gemma-4:e2b",
            "gemma-4:e2b",
            ("ggml-org/gemma-4-E2B-it-GGUF", "gemma-4-E2B-it-Q4_0.gguf"),
            "google/gemma-4-E2B-it",
        ),
    ] {
        let out = pull(name)?;
        assert!(out.status.success(), "{name}: {out:?}");
        let (family, size) = snapshot_size(entry)?;
        let pulled = ardana.entry(entry)?;
        assert_eq!(
            pulled["source"].as_str(),
            Some(format!("hf.co/{}:{}", gguf.0, gguf.1).as_str())
        );
        assert_eq!(
            pulled["weights"].as_str().map(PathBuf::from),
            Some(common::hf_file(gguf.0, gguf.1)?)
        );
        let read = pulled["tokenizer"].as_str().map(PathBuf::from);
        assert_eq!(read, Some(common::hf_file(tokenizer, "tokenizer.json")?));
        let commit = size["tokenizer"]["commit"]
            .as_str()
            .context("a tokenizer pin")?;
        assert!(
            read.is_some_and(|path| path.ends_with(format!("snapshots/{commit}/tokenizer.json"))),
            "{entry} reads its tokenizer at {commit}"
        );
        let profile = pulled["profile"].as_table().context("profile")?;
        assert_eq!(profile["name"].as_str(), Some(entry));
        assert_eq!(family["layout"], "chat");
        assert_eq!(profile["layout"]["kind"].as_str(), Some("chat"));
        assert_eq!(
            profile["release_date"].as_str(),
            size["release_date"].as_str(),
            "the library's release date"
        );
    }
    let file = ardana.models_toml()?;
    let names: Vec<&str> = file["model"]
        .as_array()
        .context("[[model]]")?
        .iter()
        .filter_map(|m| m.get("name").and_then(toml::Value::as_str))
        .collect();
    assert_eq!(
        names,
        ["decider:2b", "qwen3.5:0.8b", "smollm3:3b", "gemma-4:e2b"]
    );
    Ok(())
}

#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF in tmp/hf (cargo xtask fetch), loaded through llama.cpp"]
fn run_pulls_a_library_model_first() -> Result<()> {
    let ardana = Ardana::new("e2e-library-run")?;
    let mut cmd = ardana.command(["run", "decider:2b", "--json", "--request"]);
    cmd.arg(common::request("ticket.json"))
        .env(NO_HUB.0, NO_HUB.1);
    let out = cmd.output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("{stderr}");
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains(
            "pulling decider:2b (hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf, 1.3 GB)"
        ),
        "{stderr}"
    );
    common::check_ticket(&serde_json::from_slice(&out.stdout)?)?;
    assert_eq!(
        ardana.entry("decider:2b")?["name"].as_str(),
        Some("decider:2b")
    );

    // Pulled now: the second run pulls nothing.
    let mut cmd = ardana.command(["run", "decider:2b", "--json", "--request"]);
    cmd.arg(common::request("ticket.json"))
        .env(NO_HUB.0, NO_HUB.1);
    let out = cmd.output()?;
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("pulling"));

    // Pulled through a URL source, the model runs under another spelling without a request (Q10).
    let fresh = Ardana::new("e2e-library-run-url")?;
    let library = LibraryServer::start(&common::snapshot_bytes()?)?;
    let run = |name: &str, url: &str| -> Result<Output> {
        let mut cmd = fresh.command(["run", name, "--json", "--request"]);
        cmd.arg(common::request("ticket.json"))
            .env(NO_HUB.0, NO_HUB.1)
            .env("ARDANA_LIBRARY", url);
        Ok(cmd.output()?)
    };
    let out = run("decider", &library.url)?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(library.requests().len(), 1, "{:?}", library.requests());
    let out = run("Decider:2B-Q4_K_M", &library.url)?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(!stderr.contains("pulling"), "{stderr}");
    common::check_ticket(&serde_json::from_slice(&out.stdout)?)?;
    assert_eq!(library.requests().len(), 1, "{:?}", library.requests());
    Ok(())
}

/// R4.5, R3.2: under a URL source (a loopback server of the snapshot), a request naming a library model that is not
/// pulled pulls it first, concurrent requests under every spelling of it (Q10) sharing the one pull of its canonical
/// name, which `/health` and `ardana ps` print; the index answered the names, so no manifest was asked for.
#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF in tmp/hf (cargo xtask fetch), loaded through llama.cpp"]
fn serve_pulls_the_default_model_on_first_use() -> Result<()> {
    let ardana = Ardana::new("e2e-library-serve")?;
    let library = LibraryServer::start(&common::snapshot_bytes()?)?;
    let server = Served::start(
        &ardana,
        &[],
        &[NO_HUB, ("ARDANA_LIBRARY", library.url.as_str())],
    )?;
    let ticket = request("ticket.json")?;

    // `jev-latest` and every spelling of decider:2b at once: one pull, all answered by decider:2b.
    let answers: Vec<(u16, Value)> = std::thread::scope(|scope| {
        let asks: Vec<_> = [
            "jev-latest",
            "decider",
            "Decider:LATEST",
            "decider:2b-q4_k_m",
            "decider:2b",
        ]
        .into_iter()
        .map(|model| {
            let mut body = ticket.clone();
            body["model"] = json!(model);
            let server = &server;
            scope.spawn(move || server.decide(&body))
        })
        .collect();
        asks.into_iter()
            .map(|ask| ask.join().expect("the request thread ends"))
            .collect::<Result<_>>()
    })?;
    for (status, body) in &answers {
        assert_eq!(*status, 200, "{body}");
        common::check_ticket(body)?;
    }
    let log = server.log_text();
    assert_eq!(
        log.matches(
            "ardana serve: pulling decider:2b (hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf, 1.3 GB)"
        )
        .count(),
        1,
        "one pull for every request:\n{log}"
    );
    let (status, health) = server.get("/health", "")?;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&health)?["x_loaded"],
        json!(["decider:2b"])
    );
    let ps = ardana.ok(["ps", "--port", &server.port.to_string()])?;
    let rows: Vec<&str> = ps
        .lines()
        .skip(1)
        .filter_map(|row| row.split_whitespace().next())
        .collect();
    assert_eq!(rows, ["decider:2b"], "{ps}");
    assert_eq!(
        ardana.entry("decider:2b")?["source"].as_str(),
        Some("hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf")
    );

    // A request without a model, now that decider:2b is pulled.
    let mut bare = ticket.clone();
    bare.as_object_mut().context("request")?.remove("model");
    let (status, body) = server.decide(&bare)?;
    assert_eq!(status, 200, "{body}");
    common::check_ticket(&body)?;

    let listed = server.models()?;
    assert_eq!(listed["models"][0]["name"], "decider:2b");
    assert_eq!(listed["models"][0]["x_pulled"], true);
    assert_eq!(listed["models"][0]["x_default"], true);
    let paths: Vec<String> = library.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, ["/models.json"], "one index GET, no manifest");
    Ok(())
}

#[test]
#[ignore = "e2e: Qwen3.5-0.8B GGUF and tokenizer in tmp/hf (cargo xtask fetch), loaded through llama.cpp"]
fn serve_picks_up_models_pulled_while_it_runs() -> Result<()> {
    let ardana = Ardana::new("e2e-library-serve-live")?;
    let server = Served::start(&ardana, &[], &[NO_HUB])?;
    let pulled = |listed: &Value| -> Vec<String> {
        listed["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["x_pulled"] == true)
            .filter_map(|m| m["name"].as_str().map(String::from))
            .collect()
    };
    assert!(pulled(&server.models()?).is_empty());

    let mut cmd = ardana.command(["pull", "qwen3.5:0.8b"]);
    cmd.env(NO_HUB.0, NO_HUB.1);
    assert!(cmd.status()?.success(), "{cmd:?}");
    let listed = server.models()?;
    assert_eq!(pulled(&listed), ["qwen3.5:0.8b"]);
    assert_eq!(listed["models"][0]["x_default"], true, "{listed}");

    let mut sentiment = request("sentiment.json")?;
    sentiment["model"] = json!("qwen3.5:0.8b");
    let (status, body) = server.decide(&sentiment)?;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["model"], "qwen3.5:0.8b");
    common::check_sentiment(&body)?;
    assert!(
        !server.log_text().contains("pulling"),
        "nothing was pulled by the server"
    );

    // A name that is neither pulled nor in the library lists both.
    sentiment["model"] = json!("nope");
    let (status, body) = server.decide(&sentiment)?;
    assert_eq!(status, 404, "{body}");
    assert_eq!(
        body["detail"]["message"],
        "no model named \"nope\"; pulled: qwen3.5:0.8b; the library at https://ardana.ai/models/ is pulled on first use"
    );
    Ok(())
}

/// A loopback stand-in of Hugging Face serving the hub cache at `hub` (`tmp/hf/hub`) as the Hub serves a repository:
/// `GET /api/models/<org>/<repo>/revision/<revision>`, the commit and its files (`main`'s without `/revision/...`), and
/// `HEAD` and `GET /<org>/<repo>/resolve/<revision>/<file>`, the file with its `ETag` (its blob's name),
/// `X-Repo-Commit` and length; a revision that is not a held commit is read through `refs/<revision>`. It records every
/// request as `<METHOD> <path>`.
struct StandIn {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl StandIn {
    fn start(hub: PathBuf) -> Result<StandIn> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let (hub, seen) = (hub.clone(), seen.clone());
                std::thread::spawn(move || {
                    let _ = serve(&hub, stream, &seen);
                });
            }
        });
        Ok(StandIn { endpoint, requests })
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the request log").clone()
    }
}

/// Answers the one request on `stream` from the hub cache at `hub`, recording it in `seen`.
fn serve(hub: &Path, mut stream: std::net::TcpStream, seen: &Mutex<Vec<String>>) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }
    let mut parts = line.split(' ');
    let (method, path) = (
        parts.next().unwrap_or_default().to_string(),
        parts.next().unwrap_or_default().to_string(),
    );
    seen.lock()
        .expect("the request log")
        .push(format!("{method} {path}"));
    let path = path.split('?').next().unwrap_or_default();
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let snapshot = |org: &str, repo: &str, revision: &str| -> Option<(String, PathBuf)> {
        let dir = hub.join(format!("models--{org}--{repo}"));
        let commit = if dir.join("snapshots").join(revision).is_dir() {
            revision.to_string()
        } else {
            std::fs::read_to_string(dir.join("refs").join(revision))
                .ok()?
                .trim()
                .to_string()
        };
        let snapshot = dir.join("snapshots").join(&commit);
        snapshot.is_dir().then_some((commit, snapshot))
    };
    match segments[..] {
        ["api", "models", org, repo] | ["api", "models", org, repo, "revision", _]
            if method == "GET" =>
        {
            let revision = segments.get(5).copied().unwrap_or("main");
            if let Some((commit, dir)) = snapshot(org, repo, revision) {
                let siblings: Vec<Value> = std::fs::read_dir(&dir)?
                    .filter_map(|entry| entry.ok())
                    .map(|entry| json!({"rfilename": entry.file_name().to_string_lossy()}))
                    .collect();
                let body =
                    json!({"id": format!("{org}/{repo}"), "sha": commit, "siblings": siblings})
                        .to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
                return Ok(());
            }
        }
        [org, repo, "resolve", revision, file] => {
            if let Some((commit, dir)) = snapshot(org, repo, revision) {
                let path = dir.join(file);
                if let (Ok(blob), Ok(meta)) = (std::fs::read_link(&path), std::fs::metadata(&path))
                {
                    let etag = blob
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nETag: \"{etag}\"\r\nX-Repo-Commit: {commit}\r\nConnection: close\r\n\r\n",
                        meta.len()
                    )?;
                    if method == "GET" {
                        std::io::copy(&mut std::fs::File::open(&path)?, &mut stream)?;
                    }
                    return Ok(());
                }
            }
        }
        _ => {}
    }
    write!(
        stream,
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    Ok(())
}

/// R2.4: online, a library pull asks Hugging Face for its repositories at the commits its size pins and never at
/// `main`, and for the one GGUF its quant names (Q14): `qwen3.5:0.8b` and `gemma-4:e2b`, the GGUF repository at
/// `gguf.commit`, the tokenizer repository at `tokenizer.commit`, through a stand-in of the Hub (`HF_ENDPOINT`) serving
/// `tmp/hf`, each into an empty hub cache.
#[test]
#[ignore = "e2e: Qwen3.5-0.8B and gemma-4-E2B GGUFs and tokenizers in tmp/hf (cargo xtask fetch), served by a stand-in of the Hub"]
fn pull_reads_the_pinned_commit() -> Result<()> {
    let fetched =
        PathBuf::from(std::env::var_os("HF_HOME").context("HF_HOME is not set")?).join("hub");
    for name in ["qwen3.5:0.8b", "gemma-4:e2b"] {
        let ardana = Ardana::new(&format!("e2e-library-pinned-{}", name.replace(':', "-")))?;
        let stand_in = StandIn::start(fetched.clone())?;
        let cache = ardana.home.with_file_name("hub");
        let mut cmd = ardana.command(["pull", name]);
        cmd.env_remove("HF_HUB_OFFLINE")
            .env_remove("HF_TOKEN")
            .env("HF_ENDPOINT", &stand_in.endpoint)
            .env("HF_HUB_CACHE", &cache);
        let out = cmd.output()?;
        println!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{out:?}");

        let (_, size) = snapshot_size(name)?;
        let repo = |pin: &Value| -> Result<String> {
            let repo = pin["repo"].as_str().context("repo")?;
            Ok(repo.strip_prefix("hf.co/").unwrap_or(repo).to_string())
        };
        let (gguf, commit) = (
            repo(&size["gguf"])?,
            size["gguf"]["commit"].as_str().context("commit")?,
        );
        let (tokenizer, tokenizer_commit) = (
            repo(&size["tokenizer"])?,
            size["tokenizer"]["commit"]
                .as_str()
                .context("tokenizer commit")?,
        );
        let file = size["gguf"]["quants"][0]["file"].as_str().context("file")?;
        let pins = [
            (gguf.as_str(), commit),
            (tokenizer.as_str(), tokenizer_commit),
        ];
        let requests = stand_in.requests();
        for request in &requests {
            assert!(!request.contains("main"), "{request} names main");
            assert!(
                pins.iter().any(|(repo, pin)| {
                    request == &format!("GET /api/models/{repo}/revision/{pin}")
                        || ["HEAD", "GET"].iter().any(|method| {
                            request.starts_with(&format!("{method} /{repo}/resolve/{pin}/"))
                        })
                }),
                "{request} names no pinned commit"
            );
            assert!(
                !request.ends_with(".gguf") || request.ends_with(&format!("/{file}")),
                "{request} asks for another GGUF than {file}"
            );
        }
        for wanted in [
            format!("GET /api/models/{gguf}/revision/{commit}"),
            format!("GET /{gguf}/resolve/{commit}/{file}"),
            format!("GET /api/models/{tokenizer}/revision/{tokenizer_commit}"),
            format!("GET /{tokenizer}/resolve/{tokenizer_commit}/tokenizer.json"),
            format!("GET /{tokenizer}/resolve/{tokenizer_commit}/chat_template.jinja"),
        ] {
            assert!(requests.contains(&wanted), "{wanted} in {requests:#?}");
        }

        let pulled = ardana.entry(name)?;
        assert_eq!(
            pulled["source"].as_str(),
            Some(format!("hf.co/{gguf}:{file}").as_str())
        );
        let snapshot = |repo: &str, commit: &str| {
            cache
                .join(format!("models--{}/snapshots", repo.replace('/', "--")))
                .join(commit)
        };
        assert_eq!(
            pulled["weights"].as_str().map(PathBuf::from),
            Some(snapshot(&gguf, commit).join(file))
        );
        assert_eq!(
            pulled["tokenizer"].as_str().map(PathBuf::from),
            Some(snapshot(&tokenizer, tokenizer_commit).join("tokenizer.json"))
        );
    }
    Ok(())
}
