//! The model library as the registry reads it at run time: the snapshot (C5) is a schema 1 document of families that
//! pins every repository `cargo xtask fetch` installs; a URL source answers a name by its family's manifest, fetched
//! every time and kept in the cache, which stands in when the GET fails; `off` and a file source send nothing.
//! `<family>[:<tag>]` names one quant of a size by enumeration, case-insensitively, and stands for the file a pull
//! reads at the commits its size pins, never at `main`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use ardana_core::{LoadOptions, LoadedModel, Runtime, Runtimes};
use ardana_registry::library::{
    DEFAULT_MODEL, LIBRARY_VAR, Library, LibraryClient, LibraryDocument, LibraryPick,
    LibrarySource, MODELS_PAGE, USER_AGENT,
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

/// The manifest of the family `family` as the landing publishes it (C2), from the snapshot's family.
fn manifest(family: &str) -> Result<Vec<u8>> {
    let document: Value = serde_json::from_str(&std::fs::read_to_string(snapshot_path())?)?;
    let model = document["models"]
        .as_array()
        .context("models")?
        .iter()
        .find(|m| m["name"] == family)
        .with_context(|| format!("the snapshot has no {family}"))?;
    Ok(serde_json::to_vec_pretty(
        &json!({"schema": 1, "model": model}),
    )?)
}

/// The requests a [`Loopback`] saw: each one's path and `User-Agent`.
type Requests = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// A loopback server publishing the snapshot as the landing does: the index at `/models.json`, each family's manifest
/// at `/models/<family>.json`, 404 elsewhere, and 500 everywhere while `failing`. It records every request's path and
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

/// R2.3: the snapshot is a schema 1 document of 6 families, 12 sizes and 34 quants that reads whole, the landing's
/// document byte for byte where that checkout is beside this one; its names and defaults are the library's, every
/// repository `xtask/fetch.toml` installs is pinned at that file's revision, and cargo names it as `ARDANA_LIBRARY`
/// for every test.
#[test]
fn the_snapshot_is_a_schema_1_library_of_families() -> Result<()> {
    let path = snapshot_path();
    let text = std::fs::read_to_string(&path)?;
    let landing = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ardana-landing/src/lib/data/models.json");
    if landing.is_file() {
        assert!(
            std::fs::read_to_string(&landing)? == text,
            "{} is a copy of {}",
            path.display(),
            landing.display()
        );
    }
    let document = LibraryDocument::parse(&text, env!("CARGO_PKG_VERSION"))?;
    assert_eq!(document.schema, 1);
    let raw: Value = serde_json::from_str(&text)?;
    let count = |models: &Value| -> (usize, usize, usize) {
        let families = models.as_array().map(Vec::as_slice).unwrap_or_default();
        let sizes: Vec<&Value> = families
            .iter()
            .flat_map(|f| f["sizes"].as_array().map(Vec::as_slice).unwrap_or_default())
            .collect();
        let quants = sizes
            .iter()
            .map(|s| s["gguf"]["quants"].as_array().map_or(0, Vec::len))
            .sum();
        (families.len(), sizes.len(), quants)
    };
    assert_eq!(count(&raw["models"]), (6, 12, 34));
    assert_eq!(
        count(&serde_json::to_value(&document.models)?),
        (6, 12, 34),
        "nothing is left out"
    );
    let library = Library::from_document(document);
    assert_eq!(
        count(&serde_json::to_value(&library.models)?),
        (6, 12, 34),
        "a pull reads every size"
    );
    assert_eq!(library.default, "decider:2b");
    assert_eq!(
        library.find(DEFAULT_MODEL)?.map(|pick| pick.name()),
        Some(library.default.clone())
    );
    assert_eq!(library.browser_default.as_deref(), Some("decider:0.8b"));
    let families: Vec<(&str, &str)> = library
        .models
        .iter()
        .map(|f| (f.name.as_str(), f.latest.as_str()))
        .collect();
    assert_eq!(
        families,
        [
            ("decider", "2b"),
            ("gemma-4", "e4b"),
            ("qwen3.5", "0.8b"),
            ("qwen3.6", "35b-a3b"),
            ("qwen3.8", "27b"),
            ("smollm3", "3b")
        ]
    );
    assert_eq!(
        library.names(),
        [
            "decider:0.8b",
            "decider:2b",
            "decider:4b",
            "gemma-4:e2b",
            "gemma-4:e4b",
            "gemma-4:12b",
            "gemma-4:26b-a4b",
            "gemma-4:31b",
            "qwen3.5:0.8b",
            "qwen3.6:35b-a3b",
            "qwen3.8:27b",
            "smollm3:3b"
        ]
    );
    let sizes = library.sizes();
    let browser: Vec<(String, &str, &str, Option<&str>)> = sizes
        .iter()
        .filter_map(|pick| {
            let browser = pick.size.browser.as_ref()?;
            assert!(browser.bytes > 0, "{}", pick.name());
            assert!(browser.profile.is_object(), "{}", pick.name());
            Some((
                pick.name(),
                browser.repo.as_str(),
                browser.quant.as_str(),
                pick.size.source.as_ref().map(|s| s.repo.as_str()),
            ))
        })
        .collect();
    assert_eq!(
        browser,
        [
            (
                "decider:0.8b".to_string(),
                "hf.co/ardana-ai/decider-0.8b-ONNX",
                "int4",
                Some("hf.co/Mapika/decider-0.8b")
            ),
            (
                "decider:2b".to_string(),
                "hf.co/ardana-ai/decider-2b-ONNX",
                "int4",
                Some("hf.co/Mapika/decider-2b")
            ),
            (
                "qwen3.5:0.8b".to_string(),
                "hf.co/ardana-ai/qwen3.5-0.8b-ONNX",
                "int8",
                Some("hf.co/Qwen/Qwen3.5-0.8B")
            ),
        ]
    );
    // A browser variant without its own `decider_config.json` reads the stock profile named canonically (Q17).
    let qwen = library.browser("qwen3.5:0.8b").context("qwen3.5:0.8b")?;
    assert_eq!(
        qwen.size.browser.as_ref().map(|b| &b.profile["name"]),
        Some(&json!("qwen3.5:0.8b"))
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
    for pick in library.sizes() {
        let size = &pick.size;
        let reads = [
            Some((&size.gguf.repo, &size.gguf.commit)),
            size.source.as_ref().map(|pin| (&pin.repo, &pin.commit)),
            size.tokenizer.as_ref().map(|pin| (&pin.repo, &pin.commit)),
            size.browser.as_ref().map(|b| (&b.repo, &b.commit)),
        ];
        for (repo, commit) in reads.into_iter().flatten() {
            if let Some((_, revision)) = pins.iter().find(|(pin, _)| pin == repo) {
                assert_eq!(
                    commit,
                    revision,
                    "{} reads {repo} at its fetch.toml pin",
                    pick.name()
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

/// R2.2: a name is a family and a tag the family enumerates (Q8 to Q12), compared case-insensitively and whole; a tag
/// it does not list is an error listing its canonical tags, and a reference or another family is no library name.
#[test]
fn names_sizes_and_quants() -> Result<()> {
    let library = snapshot()?;
    let pick = |input: &str| -> LibraryPick {
        library
            .find(input)
            .unwrap_or_else(|err| panic!("{input}: {err}"))
            .unwrap_or_else(|| panic!("{input} is a library name"))
    };

    let decider = pick("decider:2b");
    assert_eq!(decider.name(), "decider:2b");
    assert_eq!(
        (decider.family.as_str(), decider.size.size.as_str()),
        ("decider", "2b")
    );
    assert_eq!(
        decider.reference(),
        "hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf"
    );
    assert_eq!(decider.quant.bytes, 1_274_396_800);
    assert_eq!(decider.size.tokenizer, None);
    assert_eq!(decider.size.layout, None);
    assert_eq!(decider.size.kind.as_deref(), Some("decider"));
    assert_eq!(
        Ref::parse(&decider.reference())?,
        Ref::Hf {
            org: "Mapika".into(),
            repo: "decider-2b-GGUF".into(),
            file: Some(HfFile::Name("decider-2b-v11-Q4_K_M.gguf".into())),
        }
    );
    for same in [
        "decider",
        "Decider:LATEST",
        "decider:2b",
        "decider:2b-q4_k_m",
    ] {
        assert_eq!(pick(same), decider, "{same}");
    }

    let q8 = pick("decider:2b-Q8_0");
    assert_eq!(q8.name(), "decider:2b-q8_0");
    assert_eq!(
        q8.reference(),
        "hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q8_0.gguf"
    );
    assert_eq!(q8.quant.bytes, 2_012_012_672);

    let moe = pick("gemma-4:26b-a4b-q8_0");
    assert_eq!(moe.name(), "gemma-4:26b-a4b-q8_0");
    assert_eq!(
        moe.reference(),
        "hf.co/ggml-org/gemma-4-26B-A4B-it-GGUF:gemma-4-26B-A4B-it-Q8_0.gguf"
    );
    assert_eq!(pick("gemma-4").name(), "gemma-4:e4b");

    let qwen = pick("qwen3.5:0.8b");
    assert_eq!(
        qwen.reference(),
        "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Qwen3.5-0.8B-Q4_0.gguf"
    );
    assert_eq!(
        qwen.size.tokenizer.as_ref().map(|t| t.repo.as_str()),
        Some("hf.co/Qwen/Qwen3.5-0.8B")
    );
    assert_eq!(qwen.size.layout, Some(LayoutKind::Chat));
    assert_eq!(
        pick("smollm3").reference(),
        "hf.co/ggml-org/SmolLM3-3B-GGUF:SmolLM3-Q4_K_M.gguf"
    );

    // Q12: a tag the family does not list names the family and lists its canonical tags in document order.
    let gemma = "e2b, e2b-q8_0, e2b-bf16, e4b, e4b-q8_0, e4b-bf16, 12b, 12b-q8_0, 12b-bf16, 26b-a4b, \
                 26b-a4b-q8_0, 26b-a4b-bf16, 31b, 31b-q8_0, 31b-bf16";
    let decider_tags = "0.8b, 2b, 2b-q8_0, 2b-bf16, 4b, 4b-q8_0, 4b-bf16";
    for (input, family, tag, tags) in [
        ("gemma-4:9b", "gemma-4", "9b", gemma),
        ("decider:2b-q5_k_m", "decider", "2b-q5_k_m", decider_tags),
        ("decider:", "decider", "", decider_tags),
        ("decider:2b:q8_0", "decider", "2b:q8_0", decider_tags),
    ] {
        let err = library.find(input).unwrap_err();
        let RegistryError::UnknownTag {
            family: named,
            tag: asked,
            tags: listed,
        } = &err
        else {
            panic!("{input}: {err:?}");
        };
        assert_eq!(
            (named.as_str(), asked.as_str(), listed.join(", ")),
            (family, tag, tags.to_string()),
            "{input}"
        );
        assert_eq!(
            err.to_string(),
            format!("{family} has no tag {tag:?}; its tags are {tags}")
        );
    }

    for input in [
        "nomodel:2b",
        "decider-2b",
        "hf.co/o/r",
        "ollama:decider",
        "./decider",
        "decider.gguf",
        "",
        " decider",
    ] {
        assert_eq!(library.find(input)?, None, "{input:?}");
    }
    // A bare name outside the library is no reference either; the error points to the library's page (Q12).
    let err = Ref::parse("nomodel").unwrap_err().to_string();
    assert!(
        err.contains(&format!("neither a library model ({MODELS_PAGE})")),
        "{err}"
    );
    assert!(!err.contains("decider"), "{err}");
    Ok(())
}

/// R2.5: under a URL source a name not pulled is answered by its family's manifest (Q16), `GET
/// …/models/<lowercased family>.json` with `User-Agent: ardana/<version>`, stored as fetched at
/// `$ARDANA_HOME/library/models/<family>.json`, fetched again on the next lookup; a 404 is the unknown-model error, and
/// a reference sends nothing.
#[test]
fn resolves_a_name_by_its_family_manifest() -> Result<()> {
    let home = home("library-manifest")?;
    let server = Loopback::start()?;
    let client = server.client(&home);
    let registry = Registry::open(&home)?;

    let library = block_on(client.lookup("decider:2b-q8_0"))?;
    let Named::Library(pick) = registry.named("decider:2b-q8_0", &library)? else {
        panic!("not pulled");
    };
    assert_eq!(pick.name(), "decider:2b-q8_0");
    assert_eq!(
        pick.reference(),
        "hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q8_0.gguf"
    );
    assert_eq!(
        server.requests(),
        [(
            "/models/decider.json".to_string(),
            Some(USER_AGENT.to_string())
        )]
    );
    assert_eq!(USER_AGENT, concat!("ardana/", env!("CARGO_PKG_VERSION")));
    let cached = home.join("library/models/decider.json");
    assert_eq!(
        std::fs::read(&cached)?,
        manifest("decider")?,
        "the bytes as fetched"
    );

    // The next lookup asks again, and any spelling of the family asks for the family's manifest.
    let library = block_on(client.lookup("Decider:LATEST"))?;
    assert_eq!(
        library.find("Decider:LATEST")?.map(|pick| pick.name()),
        Some("decider:2b".into())
    );
    assert_eq!(server.requests().len(), 2);
    assert_eq!(server.requests()[1].0, "/models/decider.json");

    // A family the library lacks is a 404, an unknown model naming the library's page and no model.
    let library = block_on(client.lookup("nope:2b"))?;
    assert_eq!(library, Library::empty());
    let err = registry.named("nope:2b", &library).unwrap_err();
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

    // The commands that send nothing read the cached manifest alone.
    let library = client.cached("DECIDER:2b-Q4_K_M")?;
    assert_eq!(
        library.find("DECIDER:2b-Q4_K_M")?.map(|pick| pick.name()),
        Some("decider:2b".into())
    );
    assert_eq!(client.cached("gemma-4")?, Library::empty());
    assert_eq!(server.requests().len(), 3);
    Ok(())
}

/// R2.5: when the GET fails (no server, a 500), the family's cached manifest answers; without one the error names the
/// URL and the cause.
#[test]
fn falls_back_to_the_cached_manifest() -> Result<()> {
    let home = home("library-fallback")?;
    let closed = "http://127.0.0.1:9/models.json";
    let client = LibraryClient::new(LibrarySource::Url(closed.into()), &home);
    let err = block_on(client.lookup("decider:2b")).unwrap_err();
    let RegistryError::Library { url, msg } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(url, "http://127.0.0.1:9/models/decider.json");
    assert!(msg.to_lowercase().contains("refused"), "{msg}");
    assert!(
        err.to_string()
            .starts_with("reading the library at http://127.0.0.1:9/models/decider.json: "),
        "{err}"
    );

    // A manifest a pull fetched earlier stands in, with the registry reading the name as before.
    let cached = home.join("library/models/decider.json");
    std::fs::create_dir_all(cached.parent().context("models")?)?;
    std::fs::write(&cached, manifest("decider")?)?;
    let library = block_on(client.lookup("decider:2b-q8_0"))?;
    let registry = Registry::open(&home)?;
    assert!(matches!(
        registry.named("decider:2b-q8_0", &library)?,
        Named::Library(pick) if pick.name() == "decider:2b-q8_0"
    ));
    // Another family has no cached copy: the error stands.
    assert!(matches!(
        block_on(client.lookup("gemma-4")),
        Err(RegistryError::Library { .. })
    ));

    // A server that answers 500 is as good as none.
    let server = Loopback::start()?;
    let client = server.client(&home);
    server.failing.store(true, Ordering::SeqCst);
    let library = block_on(client.lookup("decider:2b"))?;
    assert_eq!(
        library.names(),
        ["decider:0.8b", "decider:2b", "decider:4b"]
    );
    let err = block_on(client.lookup("gemma-4")).unwrap_err();
    assert!(
        matches!(&err, RegistryError::Library { msg, .. } if msg.contains("500")),
        "{err}"
    );
    server.failing.store(false, Ordering::SeqCst);
    let library = block_on(client.lookup("gemma-4:e2b"))?;
    assert_eq!(library.models.len(), 1);
    assert_eq!(library.models[0].name, "gemma-4");

    // A 200 whose body is no manifest is an error and leaves the good cached copy in place.
    server.garbled.store(true, Ordering::SeqCst);
    let before = std::fs::read(&cached)?;
    assert!(matches!(
        block_on(client.lookup("decider:2b")),
        Err(RegistryError::Library { .. })
    ));
    assert_eq!(std::fs::read(&cached)?, before);
    assert_eq!(client.cached("decider")?.names().len(), 3);

    // A manifest whose sizes no pull reads (Q13) is no library model, as in an index.
    let mut family: Value = serde_json::from_slice(&before)?;
    for size in family["model"]["sizes"].as_array_mut().context("sizes")? {
        size["gguf"]["commit"] = json!("main");
    }
    std::fs::write(&cached, family.to_string())?;
    assert_eq!(client.cached("decider")?.names(), [] as [&str; 0]);
    Ok(())
}

/// With `ARDANA_LIBRARY=off` a library name is an unknown model and an `hf.co/` reference is read as the reference it
/// is; a file source answers from its document; neither has a URL to ask.
#[test]
fn off_and_file_sources_send_nothing() -> Result<()> {
    let home = home("library-off")?;
    let registry = Registry::open(&home)?;
    let off = LibraryClient::new(LibrarySource::Off, &home);
    assert_eq!(off.source().manifest_url("decider"), None);
    assert_eq!(block_on(off.refresh())?, None, "nothing to read again");
    for library in [
        block_on(off.lookup("decider"))?,
        off.cached("decider")?,
        off.index()?,
    ] {
        assert_eq!(library, Library::empty());
        assert!(matches!(
            registry.named("decider", &library),
            Err(RegistryError::UnknownModel { .. })
        ));
        assert_eq!(library.find("hf.co/Mapika/decider-2b-GGUF")?, None);
    }
    assert!(matches!(
        Ref::parse("hf.co/Mapika/decider-2b-GGUF")?,
        Ref::Hf { file: None, .. }
    ));

    let file = LibraryClient::new(LibrarySource::File(snapshot_path()), &home);
    assert_eq!(file.source().manifest_url("decider"), None);
    for library in [
        block_on(file.lookup("decider"))?,
        file.cached("x")?,
        file.index()?,
        block_on(file.refresh())?.context("a file source reads its document again")?,
    ] {
        assert_eq!(library.names().len(), 12);
        assert!(matches!(
            registry.named("decider", &library)?,
            Named::Library(pick) if pick.name() == "decider:2b"
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

/// The library's `qwen3.5:0.8b`.
fn qwen(library: &Library) -> Result<LibraryPick> {
    library
        .find("qwen3.5:0.8b")?
        .context("qwen3.5:0.8b is a library size")
}

/// Offline, a library pull reads the file its quant names at the size's `gguf.commit` and its tokenizer at the
/// tokenizer's commit, never at the cache's `main`, and a pinned commit the cache lacks is the not-cached error naming
/// it (Q14). A pull reads the hub cache and `HF_HUB_OFFLINE` from the environment, which a test does not set in its
/// own process, so the test builds a cache in `$ARDANA_TMP/library-pinned/hub` holding qwen3.5:0.8b's GGUF and
/// tokenizer repositories each at its pin and at [`MAIN`], which `refs/main` names, and runs itself again in a child
/// with `HF_HUB_CACHE` at that cache and `HF_HUB_OFFLINE=1`, where it pulls.
#[test]
fn library_pulls_read_the_pinned_commit() -> Result<()> {
    let tmp = PathBuf::from(std::env::var_os("ARDANA_TMP").context("ARDANA_TMP is not set")?);
    let cache = tmp.join("library-pinned").join("hub");
    if std::env::var_os("HF_HUB_CACHE").as_deref() == Some(cache.as_os_str()) {
        return pinned_pulls(&cache);
    }
    home("library-pinned")?;
    let qwen = qwen(&snapshot()?)?;
    let tokenizer_commit = qwen
        .size
        .tokenizer
        .as_ref()
        .map(|pin| pin.commit.as_str())
        .context("qwen3.5:0.8b pins its tokenizer")?;
    // The text files as `cargo xtask fetch` put them into `tmp/hf`; the GGUFs empty, since no runtime loads them.
    let fetched = PathBuf::from(std::env::var_os("HF_HOME").context("HF_HOME is not set")?)
        .join("hub/models--Qwen--Qwen3.5-0.8B/snapshots")
        .join(tokenizer_commit);
    for (repo, pin, files) in [
        (
            "models--ggml-org--Qwen3.5-0.8B-GGUF",
            qwen.size.gguf.commit.as_str(),
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
    let qwen = qwen(&library)?;
    let tokenizer_commit = qwen
        .size
        .tokenizer
        .as_ref()
        .map(|pin| pin.commit.clone())
        .context("a tokenizer pin")?;
    let weights = cache
        .join("models--ggml-org--Qwen3.5-0.8B-GGUF/snapshots")
        .join(&qwen.size.gguf.commit);
    let tokenizer = cache
        .join("models--Qwen--Qwen3.5-0.8B/snapshots")
        .join(&tokenizer_commit);

    // The tokenizer under `snapshots/<tokenizer commit>/`, its chat template beside it; the weights at `gguf.commit`,
    // the file the quant names, recorded as that file's reference.
    let model = pull(&library, "qwen3.5")?;
    assert_eq!(model.name, "qwen3.5:0.8b");
    assert_eq!(
        model.source,
        "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Qwen3.5-0.8B-Q4_0.gguf"
    );
    assert_eq!(model.tokenizer, tokenizer.join("tokenizer.json"));
    assert_eq!(model.weights, weights.join("Qwen3.5-0.8B-Q4_0.gguf"));
    assert_eq!(model.profile.name, "qwen3.5:0.8b");

    // Another quant's file, under `gguf.commit` too.
    let q8 = pull(&library, "qwen3.5:0.8b-Q8_0")?;
    assert_eq!(q8.name, "qwen3.5:0.8b-q8_0");
    assert_eq!(q8.weights, weights.join("Qwen3.5-0.8B-Q8_0.gguf"));
    assert_eq!(q8.tokenizer, tokenizer.join("tokenizer.json"));

    // A pinned commit the cache lacks is not cached, named, though `refs/main` names a snapshot the cache holds.
    for (repo, pin) in [
        ("hf.co/Qwen/Qwen3.5-0.8B", Pin::Tokenizer),
        ("hf.co/ggml-org/Qwen3.5-0.8B-GGUF", Pin::Weights),
    ] {
        let mut unheld = library.clone();
        let size = unheld
            .models
            .iter_mut()
            .find(|f| f.name == "qwen3.5")
            .and_then(|f| f.sizes.first_mut())
            .context("qwen3.5:0.8b")?;
        match pin {
            Pin::Tokenizer => {
                size.tokenizer.as_mut().context("a tokenizer pin")?.commit = UNHELD.into()
            }
            Pin::Weights => size.gguf.commit = UNHELD.into(),
        }
        let err = pull(&unheld, "qwen3.5:0.8b").unwrap_err();
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

/// Which pin of a size [`pinned_pulls`] moves to a commit the cache lacks.
enum Pin {
    Weights,
    Tokenizer,
}
