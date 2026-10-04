//! The model library as the registry reads it at run time: the snapshot (C5) is a schema 1 document that pins every
//! repository `cargo xtask fetch` installs; a URL source answers a name by its manifest, fetched every time and kept in
//! the cache, which stands in when the GET fails; `off` and a file source send nothing. `<name>` and `<name>:<quant>`
//! name library models case-insensitively and stand for the `hf.co/` reference they pull, read at the commits their
//! entry pins, never at `main`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use ardana_core::{LoadOptions, LoadedModel, Runtime, Runtimes};
use ardana_registry::library::{
    DEFAULT_MODEL, LIBRARY_VAR, Library, LibraryClient, LibraryDocument, LibrarySource,
    MODELS_PAGE, USER_AGENT,
};
use ardana_registry::refs::{HfFile, Ref};
use ardana_registry::{LayoutKind, Named, PullOptions, Registry, RegistryError};
use serde_json::{Value, json};

/// The snapshot (C5), beside these tests.
fn snapshot_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/models.json")
}

fn snapshot() -> Result<Library> {
    Ok(Library::from_file(&snapshot_path())?)
}

/// `$ARDANA_TMP/<test>/home`, emptied: an Ardana home of the test's own.
fn home(test: &str) -> Result<PathBuf> {
    let tmp = PathBuf::from(std::env::var_os("ARDANA_TMP").context("ARDANA_TMP is not set")?);
    let dir = tmp.join(test);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("home"))
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

/// The manifest of `name` as the landing publishes it (C2), from the snapshot's entry.
fn manifest(name: &str) -> Result<Vec<u8>> {
    let document: Value = serde_json::from_str(&std::fs::read_to_string(snapshot_path())?)?;
    let entry = document["models"]
        .as_array()
        .context("models")?
        .iter()
        .find(|m| m["name"] == name)
        .with_context(|| format!("the snapshot has no {name}"))?;
    Ok(serde_json::to_vec_pretty(
        &json!({"schema": 1, "model": entry}),
    )?)
}

/// The requests a [`Loopback`] saw: each one's path and `User-Agent`.
type Requests = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// A loopback server publishing the snapshot as the landing does: the index at `/models.json`, each model's manifest
/// at `/models/<name>.json`, 404 elsewhere, and 500 everywhere while `failing`. It records every request's path and
/// `User-Agent`.
struct Loopback {
    index_url: String,
    requests: Requests,
    failing: Arc<AtomicBool>,
    /// Answers 200 with a body that is no manifest.
    garbled: Arc<AtomicBool>,
}

impl Loopback {
    fn start() -> Result<Loopback> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let requests: Requests = Arc::default();
        let failing = Arc::new(AtomicBool::new(false));
        let garbled = Arc::new(AtomicBool::new(false));
        let (seen, fail, garble) = (requests.clone(), failing.clone(), garbled.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let path = head
                    .lines()
                    .next()
                    .and_then(|line| line.split(' ').nth(1))
                    .unwrap_or_default()
                    .to_string();
                let agent = head.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("user-agent")
                        .then(|| value.trim().to_string())
                });
                seen.lock().unwrap().push((path.clone(), agent));
                let (status, body) = if fail.load(Ordering::SeqCst) {
                    ("500 Internal Server Error", Vec::new())
                } else if garble.load(Ordering::SeqCst) {
                    ("200 OK", b"<html>a captive portal</html>".to_vec())
                } else if path == "/models.json" {
                    ("200 OK", std::fs::read(snapshot_path()).unwrap_or_default())
                } else {
                    match path
                        .strip_prefix("/models/")
                        .and_then(|file| file.strip_suffix(".json"))
                        .and_then(|name| manifest(name).ok())
                    {
                        Some(body) => ("200 OK", body),
                        None => ("404 Not Found", b"not found".to_vec()),
                    }
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        Ok(Loopback {
            index_url: format!("http://127.0.0.1:{port}/models.json"),
            requests,
            failing,
            garbled,
        })
    }

    fn requests(&self) -> Vec<(String, Option<String>)> {
        self.requests.lock().unwrap().clone()
    }

    fn client(&self, home: &Path) -> LibraryClient {
        LibraryClient::new(LibrarySource::Url(self.index_url.clone()), home)
    }
}

/// R2.2: the snapshot is a schema 1 document whose every entry this version reads, its names and defaults are the
/// library's, every repository `xtask/fetch.toml` installs is pinned at that file's revision, and cargo names it as
/// `ARDANA_LIBRARY` for every test.
#[test]
fn the_snapshot_is_a_schema_1_library() -> Result<()> {
    let path = snapshot_path();
    let text = std::fs::read_to_string(&path)?;
    let document = LibraryDocument::parse(&text, env!("CARGO_PKG_VERSION"))?;
    assert_eq!(document.schema, 1);
    let raw: Value = serde_json::from_str(&text)?;
    assert_eq!(
        raw["models"].as_array().map(Vec::len),
        Some(document.models.len()),
        "every entry reads"
    );
    let library = Library::from_document(document);
    assert_eq!(library.default, DEFAULT_MODEL);
    assert_eq!(library.browser_default.as_deref(), Some("decider-0.8b"));
    assert_eq!(
        library.names(),
        [
            "decider-2b",
            "decider-0.8b",
            "decider-4b",
            "qwen3.5-0.8b",
            "smollm3-3b",
            "gemma-4-e2b",
            "gemma-4-e4b",
            "gemma-4-12b",
            "gemma-4-26b-a4b",
            "gemma-4-31b",
            "qwen3.6-35b-a3b",
            "qwen3.8-27b"
        ]
    );
    let browser: Vec<(&str, &str, &str, Option<&str>)> = library
        .models
        .iter()
        .filter_map(|m| {
            let browser = m.browser.as_ref()?;
            assert!(browser.size > 0, "{}", m.name);
            assert!(browser.profile.is_object(), "{}", m.name);
            Some((
                m.name.as_str(),
                browser.weights.as_str(),
                browser.quant.as_str(),
                m.source.as_deref(),
            ))
        })
        .collect();
    assert_eq!(
        browser,
        [
            (
                "decider-2b",
                "hf.co/ardana-ai/decider-2b-ONNX",
                "int4",
                Some("hf.co/Mapika/decider-2b@533964dae8be954c5b5e19fa4948e48408094c1e")
            ),
            (
                "decider-0.8b",
                "hf.co/ardana-ai/decider-0.8b-ONNX",
                "int4",
                Some("hf.co/Mapika/decider-0.8b@a0a01d6f8135298f400a8c856b355793012ae971")
            ),
            (
                "qwen3.5-0.8b",
                "hf.co/ardana-ai/qwen3.5-0.8b-ONNX",
                "int8",
                Some("hf.co/Qwen/Qwen3.5-0.8B@2fc06364715b967f1860aea9cf38778875588b17")
            ),
        ]
    );

    // Every `[[hf]]` repository of `xtask/fetch.toml` is read at the revision it installs.
    let fetch: toml::Table = toml::from_str(&std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/fetch.toml"),
    )?)?;
    let pins: Vec<(String, String)> = fetch["hf"]
        .as_array()
        .context("[[hf]]")?
        .iter()
        .map(|entry| {
            (
                format!("hf.co/{}", entry["repo"].as_str().unwrap_or_default()),
                entry["revision"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let mut pinned = 0;
    for model in &library.models {
        let mut reads = vec![(&model.weights, &model.commit)];
        if let (Some(repo), Some(commit)) = (&model.tokenizer, &model.tokenizer_commit) {
            reads.push((repo, commit));
        }
        if let Some(browser) = &model.browser {
            reads.push((&browser.weights, &browser.commit));
        }
        for (repo, commit) in reads {
            if let Some((_, revision)) = pins.iter().find(|(pin, _)| pin == repo) {
                assert_eq!(
                    commit, revision,
                    "{} reads {repo} at its fetch.toml pin",
                    model.name
                );
                pinned += 1;
            }
        }
    }
    assert!(pinned >= 10, "{pinned} repositories pinned by fetch.toml");

    let named = PathBuf::from(std::env::var_os(LIBRARY_VAR).context("cargo sets ARDANA_LIBRARY")?);
    assert_eq!(named.canonicalize()?, path.canonicalize()?);
    assert_eq!(LibrarySource::from_env(), LibrarySource::File(named));
    Ok(())
}

#[test]
fn names_and_quants() -> Result<()> {
    let library = snapshot()?;
    let pick = |input: &str| {
        library
            .find(input)
            .unwrap_or_else(|| panic!("{input} is a library model"))
    };

    let decider = pick("decider-2b");
    assert_eq!(decider.name(), "decider-2b");
    assert_eq!(decider.reference(), "hf.co/Mapika/decider-2b-GGUF:Q4_K_M");
    assert_eq!(decider.size(), Some(1_274_396_800));
    assert_eq!(decider.model.tokenizer, None);
    assert_eq!(decider.model.layout, None);
    for same in ["Decider-2B", "decider-2b:q4_k_m", "DECIDER-2B:Q4_K_M"] {
        assert_eq!(pick(same), decider, "{same}");
    }

    let q8 = pick("decider-2b:Q8_0");
    assert_eq!(q8.name(), "decider-2b:q8_0");
    assert_eq!(q8.reference(), "hf.co/Mapika/decider-2b-GGUF:q8_0");
    assert_eq!(q8.size(), None, "only the default quant's size is known");
    assert_eq!(
        Ref::parse(&q8.reference())?,
        Ref::Hf {
            org: "Mapika".into(),
            repo: "decider-2b-GGUF".into(),
            file: Some(HfFile::Quant("q8_0".into())),
        }
    );

    let qwen = pick("qwen3.5-0.8b");
    assert_eq!(qwen.reference(), "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0");
    assert_eq!(
        qwen.model.tokenizer.as_deref(),
        Some("hf.co/Qwen/Qwen3.5-0.8B")
    );
    assert_eq!(qwen.model.layout, Some(LayoutKind::Chat));
    let smollm = pick("smollm3-3b");
    assert_eq!(smollm.reference(), "hf.co/ggml-org/SmolLM3-3B-GGUF:Q4_K_M");
    assert_eq!(
        smollm.model.tokenizer.as_deref(),
        Some("hf.co/HuggingFaceTB/SmolLM3-3B")
    );
    assert_eq!(
        pick("decider-4b").reference(),
        "hf.co/Mapika/decider-4b-GGUF:Q4_K_M"
    );
    assert_eq!(library.find(&library.default), Some(decider));

    for input in [
        "",
        "decider",
        "decider-2b-GGUF",
        "decider-2b:",
        "decider-2b:Q4 K",
        "decider-2b:a:b",
        "llama3.2",
        "hf.co/Mapika/decider-2b-GGUF",
        "ollama:decider-2b",
        "./decider-2b",
        " decider-2b",
    ] {
        assert_eq!(library.find(input), None, "{input:?}");
    }
    // A bare name outside the library is no reference either; the error points to the library's page (Q11).
    let err = Ref::parse("decider").unwrap_err().to_string();
    assert!(
        err.contains(&format!("neither a library model ({MODELS_PAGE})")),
        "{err}"
    );
    assert!(!err.contains("decider-2b"), "{err}");
    Ok(())
}

/// R2.4: under a URL source a name not pulled is answered by its manifest, `GET …/models/<lowercased name>.json` with
/// `User-Agent: ardana/<version>`, stored as fetched at `$ARDANA_HOME/library/models/<name>.json`, fetched again on
/// the next lookup; a 404 is the unknown-model error, and a reference sends nothing.
#[test]
fn resolves_a_name_by_its_manifest() -> Result<()> {
    let home = home("library-manifest")?;
    let server = Loopback::start()?;
    let client = server.client(&home);
    let registry = Registry::open(&home)?;

    let library = block_on(client.lookup("Decider-2B"))?;
    let Named::Library(pick) = registry.named("Decider-2B", &library)? else {
        panic!("not pulled");
    };
    assert_eq!(pick.name(), "decider-2b");
    assert_eq!(pick.reference(), "hf.co/Mapika/decider-2b-GGUF:Q4_K_M");
    assert_eq!(
        server.requests(),
        [(
            "/models/decider-2b.json".to_string(),
            Some(USER_AGENT.to_string())
        )]
    );
    assert_eq!(USER_AGENT, concat!("ardana/", env!("CARGO_PKG_VERSION")));
    let cached = home.join("library/models/decider-2b.json");
    assert_eq!(
        std::fs::read(&cached)?,
        manifest("decider-2b")?,
        "the bytes as fetched"
    );

    // The next lookup asks again, and another quant asks for the model's manifest.
    let library = block_on(client.lookup("decider-2b:Q8_0"))?;
    assert_eq!(
        library.find("decider-2b:Q8_0").map(|pick| pick.name()),
        Some("decider-2b:q8_0".into())
    );
    assert_eq!(server.requests().len(), 2);
    assert_eq!(server.requests()[1].0, "/models/decider-2b.json");

    // A name the library lacks is a 404, an unknown model naming the library's page and no model (Q11).
    let library = block_on(client.lookup("nope"))?;
    assert_eq!(library, Library::empty());
    let err = registry.named("nope", &library).unwrap_err();
    assert!(matches!(err, RegistryError::UnknownModel { .. }), "{err:?}");
    let text = err.to_string();
    assert!(
        text.contains(MODELS_PAGE) && !text.contains("decider"),
        "{text}"
    );
    assert_eq!(server.requests()[2].0, "/models/nope.json");
    assert!(!home.join("library/models/nope.json").exists());

    // A reference or a path is no library name: nothing is asked.
    for input in [
        "hf.co/Mapika/decider-2b-GGUF",
        "ollama:llama3.2",
        "./m.gguf",
        "My-Model.GGUF",
        "",
    ] {
        assert_eq!(block_on(client.lookup(input))?, Library::empty(), "{input}");
    }
    assert_eq!(server.requests().len(), 3);

    // The commands that send nothing read the cached manifest alone (Q8).
    let library = client.cached("DECIDER-2B:q4_k_m")?;
    assert_eq!(
        library.find("DECIDER-2B:q4_k_m").map(|pick| pick.name()),
        Some("decider-2b".into())
    );
    assert_eq!(client.cached("decider-4b")?, Library::empty());
    assert_eq!(server.requests().len(), 3);
    Ok(())
}

/// R2.5: when the GET fails (no server, a 500), the cached manifest answers; without one the error names the URL and
/// the cause.
#[test]
fn falls_back_to_the_cached_manifest() -> Result<()> {
    let home = home("library-fallback")?;
    let closed = "http://127.0.0.1:9/models.json";
    let client = LibraryClient::new(LibrarySource::Url(closed.into()), &home);
    let err = block_on(client.lookup("decider-2b")).unwrap_err();
    let RegistryError::Library { url, msg } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(url, "http://127.0.0.1:9/models/decider-2b.json");
    assert!(msg.to_lowercase().contains("refused"), "{msg}");
    assert!(
        err.to_string()
            .starts_with("reading the library at http://127.0.0.1:9/models/decider-2b.json: "),
        "{err}"
    );

    // A manifest a pull fetched earlier stands in, with the registry reading the name as before.
    let cached = home.join("library/models/decider-2b.json");
    std::fs::create_dir_all(cached.parent().context("models")?)?;
    std::fs::write(&cached, manifest("decider-2b")?)?;
    let library = block_on(client.lookup("Decider-2B"))?;
    let registry = Registry::open(&home)?;
    assert!(matches!(
        registry.named("Decider-2B", &library)?,
        Named::Library(pick) if pick.name() == "decider-2b"
    ));
    // Another name has no cached copy: the error stands.
    assert!(matches!(
        block_on(client.lookup("decider-4b")),
        Err(RegistryError::Library { .. })
    ));

    // A server that answers 500 is as good as none.
    let server = Loopback::start()?;
    let client = server.client(&home);
    server.failing.store(true, Ordering::SeqCst);
    let library = block_on(client.lookup("decider-2b"))?;
    assert_eq!(library.names(), ["decider-2b"]);
    let err = block_on(client.lookup("decider-4b")).unwrap_err();
    assert!(
        matches!(&err, RegistryError::Library { msg, .. } if msg.contains("500")),
        "{err}"
    );
    server.failing.store(false, Ordering::SeqCst);
    let library = block_on(client.lookup("decider-4b"))?;
    assert_eq!(library.names(), ["decider-4b"]);

    // A 200 whose body is no manifest is an error and leaves the good cached copy in place.
    server.garbled.store(true, Ordering::SeqCst);
    let before = std::fs::read(&cached)?;
    assert!(matches!(
        block_on(client.lookup("decider-2b")),
        Err(RegistryError::Library { .. })
    ));
    assert_eq!(std::fs::read(&cached)?, before);
    assert_eq!(client.cached("decider-2b")?.names(), ["decider-2b"]);

    // A manifest whose entry no pull reads (Q12) is no library model, as in an index.
    let mut entry: Value = serde_json::from_slice(&before)?;
    entry["model"]["commit"] = json!("main");
    std::fs::write(&cached, entry.to_string())?;
    assert_eq!(client.cached("decider-2b")?.names(), [] as [&str; 0]);
    Ok(())
}

/// R2.6: with `ARDANA_LIBRARY=off` a library name is an unknown model and an `hf.co/` reference is read as the
/// reference it is; a file source answers from its document; neither has a URL to ask.
#[test]
fn off_and_file_sources_send_nothing() -> Result<()> {
    let home = home("library-off")?;
    let registry = Registry::open(&home)?;
    let off = LibraryClient::new(LibrarySource::Off, &home);
    assert_eq!(off.source().manifest_url("decider-2b"), None);
    assert_eq!(block_on(off.refresh())?, None, "nothing to read again");
    for library in [
        block_on(off.lookup("decider-2b"))?,
        off.cached("decider-2b")?,
        off.index()?,
    ] {
        assert_eq!(library, Library::empty());
        assert!(matches!(
            registry.named("decider-2b", &library),
            Err(RegistryError::UnknownModel { .. })
        ));
        assert_eq!(library.find("hf.co/Mapika/decider-2b-GGUF"), None);
    }
    assert!(matches!(
        Ref::parse("hf.co/Mapika/decider-2b-GGUF")?,
        Ref::Hf { file: None, .. }
    ));

    let file = LibraryClient::new(LibrarySource::File(snapshot_path()), &home);
    assert_eq!(file.source().manifest_url("decider-2b"), None);
    for library in [
        block_on(file.lookup("decider-2b"))?,
        file.cached("x")?,
        file.index()?,
        block_on(file.refresh())?.context("a file source reads its document again")?,
    ] {
        assert_eq!(library.names().len(), 12);
        assert!(matches!(
            registry.named("decider-2b", &library)?,
            Named::Library(pick) if pick.name() == "decider-2b"
        ));
    }
    ensure!(!home.exists(), "a file source writes no cache");
    Ok(())
}

/// A runtime that supports any weights and loads none: a pull only asks which runtime supports its weights.
struct AnyWeights;

impl Runtime for AnyWeights {
    fn id(&self) -> &'static str {
        "any"
    }

    fn supports(&self, _weights: &Path) -> bool {
        true
    }

    fn load(&self, _weights: &Path, _opts: &LoadOptions) -> Result<Box<dyn LoadedModel>> {
        anyhow::bail!("this runtime loads nothing")
    }
}

/// The commit `refs/main` names in the cache [`library_pulls_read_the_pinned_commit`] builds, held beside each pin.
const MAIN: &str = "0000000000000000000000000000000000000000";
/// A commit that cache does not hold.
const UNHELD: &str = "1111111111111111111111111111111111111111";

/// R3.1, R3.3: offline, a library pull reads its weights at the entry's `commit` and its tokenizer at its
/// `tokenizer_commit`, never at the cache's `main`, and a pinned commit the cache lacks is the not-cached error naming
/// it. A pull reads the hub cache and `HF_HUB_OFFLINE` from the environment, which a test does not set in its own
/// process, so the test builds a cache in `$ARDANA_TMP/library-pinned/hub` holding qwen3.5-0.8b's GGUF and tokenizer
/// repositories each at its pin and at [`MAIN`], which `refs/main` names, and runs itself again in a child with
/// `HF_HUB_CACHE` at that cache and `HF_HUB_OFFLINE=1`, where it pulls.
#[test]
fn library_pulls_read_the_pinned_commit() -> Result<()> {
    let tmp = PathBuf::from(std::env::var_os("ARDANA_TMP").context("ARDANA_TMP is not set")?);
    let cache = tmp.join("library-pinned").join("hub");
    if std::env::var_os("HF_HUB_CACHE").as_deref() == Some(cache.as_os_str()) {
        return pinned_pulls(&cache);
    }
    home("library-pinned")?;
    let library = snapshot()?;
    let qwen = library
        .find("qwen3.5-0.8b")
        .context("qwen3.5-0.8b is a library model")?
        .model;
    let tokenizer_commit = qwen
        .tokenizer_commit
        .as_deref()
        .context("qwen3.5-0.8b pins its tokenizer")?;
    // The text files as `cargo xtask fetch` put them into `tmp/hf`; the GGUFs empty, since no runtime loads them.
    let fetched = PathBuf::from(std::env::var_os("HF_HOME").context("HF_HOME is not set")?)
        .join("hub/models--Qwen--Qwen3.5-0.8B/snapshots")
        .join(tokenizer_commit);
    for (repo, pin, files) in [
        (
            "models--ggml-org--Qwen3.5-0.8B-GGUF",
            qwen.commit.as_str(),
            &["Qwen3.5-0.8B-Q4_0.gguf", "Qwen3.5-0.8B-Q8_0.gguf"][..],
        ),
        (
            "models--Qwen--Qwen3.5-0.8B",
            tokenizer_commit,
            &[
                "tokenizer.json",
                "tokenizer_config.json",
                "chat_template.jinja",
            ][..],
        ),
    ] {
        let repo = cache.join(repo);
        for commit in [pin, MAIN] {
            let snapshot = repo.join("snapshots").join(commit);
            std::fs::create_dir_all(&snapshot)?;
            for file in files {
                if file.ends_with(".gguf") {
                    std::fs::write(snapshot.join(file), b"")?;
                } else {
                    let source = fetched.join(file);
                    std::fs::copy(&source, snapshot.join(file)).with_context(|| {
                        format!("{}: run `cargo xtask fetch`", source.display())
                    })?;
                }
            }
        }
        std::fs::create_dir_all(repo.join("refs"))?;
        std::fs::write(repo.join("refs/main"), MAIN)?;
    }

    let test = "library_pulls_read_the_pinned_commit";
    let out = Command::new(std::env::current_exe()?)
        .args([test, "--exact", "--nocapture"])
        .env("HF_HUB_CACHE", &cache)
        .env("HF_HUB_OFFLINE", "1")
        .env("HF_ENDPOINT", "http://127.0.0.1:9")
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    println!("{stdout}{}", String::from_utf8_lossy(&out.stderr));
    ensure!(out.status.success(), "the child failed: {}", out.status);
    ensure!(stdout.contains("1 passed"), "the child ran {test}");
    Ok(())
}

/// The child's half of [`library_pulls_read_the_pinned_commit`]: pulls through the hub cache at `cache`, offline.
fn pinned_pulls(cache: &Path) -> Result<()> {
    let library = snapshot()?;
    let runtimes = Runtimes(vec![Box::new(AnyWeights)]);
    let pull = |library: &Library, name: &str| {
        block_on(ardana_registry::pull(
            name,
            &PullOptions::default(),
            &runtimes,
            library,
        ))
    };
    let qwen = library
        .find("qwen3.5-0.8b")
        .context("qwen3.5-0.8b is a library model")?
        .model;
    let tokenizer_commit = qwen.tokenizer_commit.clone().context("a tokenizer pin")?;
    let weights = cache
        .join("models--ggml-org--Qwen3.5-0.8B-GGUF/snapshots")
        .join(&qwen.commit);
    let tokenizer = cache
        .join("models--Qwen--Qwen3.5-0.8B/snapshots")
        .join(&tokenizer_commit);

    // R3.1: the tokenizer under `snapshots/<tokenizer_commit>/`, its chat template beside it; the weights at `commit`.
    let model = pull(&library, "qwen3.5-0.8b")?;
    assert_eq!(model.tokenizer, tokenizer.join("tokenizer.json"));
    assert_eq!(model.weights, weights.join("Qwen3.5-0.8B-Q4_0.gguf"));
    assert_eq!(model.profile.name, "qwen3.5-0.8b");

    // R3.3: another quant's file, under the entry's `commit` too.
    let q8 = pull(&library, "qwen3.5-0.8b:Q8_0")?;
    assert_eq!(q8.name, "qwen3.5-0.8b:q8_0");
    assert_eq!(q8.weights, weights.join("Qwen3.5-0.8B-Q8_0.gguf"));
    assert_eq!(q8.tokenizer, tokenizer.join("tokenizer.json"));

    // A pinned commit the cache lacks is not cached, named, though `refs/main` names a snapshot the cache holds.
    for (repo, pin) in [
        ("hf.co/Qwen/Qwen3.5-0.8B", Pin::Tokenizer),
        ("hf.co/ggml-org/Qwen3.5-0.8B-GGUF", Pin::Weights),
    ] {
        let mut unheld = library.clone();
        let entry = unheld
            .models
            .iter_mut()
            .find(|m| m.name == "qwen3.5-0.8b")
            .context("qwen3.5-0.8b")?;
        match pin {
            Pin::Tokenizer => entry.tokenizer_commit = Some(UNHELD.into()),
            Pin::Weights => entry.commit = UNHELD.into(),
        }
        let err = pull(&unheld, "qwen3.5-0.8b").unwrap_err();
        assert!(
            matches!(&err, RegistryError::NotCached { what, cache: at }
                if *what == format!("{repo} at commit {UNHELD}") && at == cache),
            "{err:?}"
        );
        assert!(
            err.to_string().starts_with(&format!(
                "{repo} at commit {UNHELD} is not in the Hugging Face cache at {}",
                cache.display()
            )),
            "{err}"
        );
    }
    Ok(())
}

/// Which pin of an entry [`pinned_pulls`] moves to a commit the cache lacks.
enum Pin {
    Weights,
    Tokenizer,
}
