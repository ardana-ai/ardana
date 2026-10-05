//! R5.8 on the binary: `ardana serve` binds 127.0.0.1 and takes its key from `ARDANA_API_KEY`; with nothing pulled it
//! lists the library models it pulls on first use. R4.1 to R4.3, R4.6 and R4.7: it reads the library from its URL on
//! start and every `--library-refresh`, keeps serving when the library is unreachable, and serves the pulled models
//! alone under `ARDANA_LIBRARY=off`.

mod common;

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use common::{Ardana, LibraryServer, Seen, Served, eventually};
use serde_json::{Value, json};

/// The model names a `/v1/models` body lists, in order.
fn names(listed: &Value) -> Vec<String> {
    listed["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["name"].as_str().map(String::from))
        .collect()
}

/// `http://127.0.0.1:9/models.json`: a library no one answers for.
const CLOSED: &str = "http://127.0.0.1:9/models.json";

/// How long a `--library-refresh 1s` server may take to list a published change.
const REFRESHED: Duration = Duration::from_secs(10);

/// The one model `ardana pull` records without weights to download: `ollama:local`, a header-only GGUF in a fake Ollama
/// store beside the home, over Qwen3.5's tokenizer from `tmp/hf`.
fn pull_local(ardana: &Ardana) -> Result<()> {
    let store = ardana
        .home
        .parent()
        .context("the home has a parent")?
        .join("ollama");
    let manifests = store.join("manifests/registry.ollama.ai/library/local");
    std::fs::create_dir_all(&manifests)?;
    std::fs::create_dir_all(store.join("blobs"))?;
    let hex = "0".repeat(64);
    let mut blob = b"GGUF".to_vec();
    blob.extend(3u32.to_le_bytes());
    blob.extend(0u64.to_le_bytes());
    blob.extend(0u64.to_le_bytes());
    std::fs::write(store.join("blobs").join(format!("sha256-{hex}")), &blob)?;
    std::fs::write(
        manifests.join("latest"),
        json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "layers": [{
                "mediaType": "application/vnd.ollama.image.model",
                "digest": format!("sha256:{hex}"),
                "size": blob.len(),
            }],
        })
        .to_string(),
    )?;
    let mut pull = ardana.command([
        "pull",
        "ollama:local",
        "--tokenizer",
        "hf.co/Qwen/Qwen3.5-0.8B",
    ]);
    pull.env("OLLAMA_MODELS", &store);
    let out = pull.output()?;
    anyhow::ensure!(
        out.status.success(),
        "{pull:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

#[test]
fn serve_binds_localhost_and_reads_the_key_from_the_environment() -> Result<()> {
    let ardana = Ardana::new("serve-api-key-env")?;
    let server = Served::start(&ardana, &[], &[("ARDANA_API_KEY", "serve-test-key")])?;
    let banner = server.log_text();
    assert!(
        banner.contains(&format!("listening on http://127.0.0.1:{} ", server.port))
            && banner.contains("no models pulled yet; the first request pulls decider:2b ")
            && banner.contains("API key required"),
        "{banner}"
    );

    assert_eq!(server.get("/health", "")?.0, 200);
    let (status, body) = server.get("/v1/models", "")?;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("authentication_error"), "{body}");
    assert_eq!(
        server
            .get("/v1/models", "Authorization: Bearer wrong\r\n")?
            .0,
        401
    );
    let (status, body) = server.get("/v1/models", "Authorization: Bearer serve-test-key\r\n")?;
    assert_eq!(status, 200, "{body}");
    let listed: Value = serde_json::from_str(&body)?;
    let models = listed["models"].as_array().context("models")?;
    let names: Vec<&str> = models.iter().filter_map(|m| m["name"].as_str()).collect();
    assert_eq!(
        names,
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
    assert!(models.iter().all(|m| m["x_pulled"] == false), "{body}");
    assert_eq!(models[1]["x_default"], true, "{body}");
    Ok(())
}

/// `ardana serve` has one mode: `--public` is clap's unexpected argument, and `ARDANA_PUBLIC` is not read, so a
/// decision is resolved as on any server, here a 404 listing the models it could run.
#[test]
fn serve_has_no_public_mode() -> Result<()> {
    let ardana = Ardana::new("serve-no-public")?;
    let refused = ardana.output(["serve", "--public"])?;
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("unexpected argument '--public' found"),
        "{stderr}"
    );

    let server = Served::start(&ardana, &[], &[("ARDANA_PUBLIC", "1")])?;
    let banner = server.log_text();
    assert!(
        banner.contains("no models pulled yet; the first request pulls decider:2b "),
        "{banner}"
    );
    let (status, body) = server.decide(&json!({"model": "nope", "state": "s", "questions": {}}))?;
    assert_eq!(status, 404, "{body}");
    assert_eq!(
        body["detail"]["message"],
        "no model named \"nope\"; none pulled yet; the library at https://ardana.ai/models/ is pulled on first use"
    );
    assert_eq!(server.models()?["models"][1]["x_default"], true);
    Ok(())
}

/// R4.1: with an empty cache and a reachable URL source, the first `GET /v1/models` lists the document's models, and
/// the cache holds the index as fetched with its `ETag`; the one GET carries the User-Agent and nothing to revalidate.
#[test]
fn serve_fetches_the_library_on_start() -> Result<()> {
    let ardana = Ardana::new("serve-library-start")?;
    let document = common::snapshot_bytes()?;
    let library = LibraryServer::start(&document)?;
    let server = Served::start(&ardana, &[], &[("ARDANA_LIBRARY", &library.url)])?;
    assert!(
        server
            .log_text()
            .contains("no models pulled yet; the first request pulls decider "),
        "{}",
        server.log_text()
    );

    let listed = server.models()?;
    assert_eq!(names(&listed), common::snapshot_names()?);
    let models = listed["models"].as_array().context("models")?;
    for model in models {
        assert_eq!(model["x_pulled"], false, "{model}");
        assert!(model["x_size"].as_u64().is_some_and(|n| n > 0), "{model}");
    }
    assert_eq!(models[1]["name"], "decider:2b");
    assert_eq!(models[1]["x_default"], true);
    assert!(models[1]["x_browser"].as_u64().is_some_and(|n| n > 0));
    let browser_default = models
        .iter()
        .find(|m| m["x_browser_default"] == true)
        .context("a browser default")?;
    assert_eq!(browser_default["name"], "decider:0.8b");
    assert!(browser_default["x_browser"].as_u64().is_some());

    assert_eq!(
        std::fs::read(ardana.home.join("library/models.json"))?,
        document,
        "the bytes as fetched"
    );
    assert_eq!(
        std::fs::read_to_string(ardana.home.join("library/models.json.etag"))?,
        library.etag()
    );
    assert_eq!(
        library.requests(),
        [Seen {
            path: "/models.json".into(),
            if_none_match: None,
            user_agent: Some(format!("ardana/{}", env!("CARGO_PKG_VERSION"))),
        }]
    );

    // A fresh home serves a library `--default-model` its empty cache does not know yet.
    let fresh = Ardana::new("serve-library-start-default")?;
    let server = Served::start(
        &fresh,
        &["--default-model", "decider:0.8b"],
        &[("ARDANA_LIBRARY", &library.url)],
    )?;
    let listed = server.models()?;
    let default = listed["models"]
        .as_array()
        .context("models")?
        .iter()
        .find(|m| m["x_default"] == true)
        .context("a default model")?;
    assert_eq!(default["name"], "decider:0.8b", "{listed}");
    Ok(())
}

/// R4.2, R3.6: with `--library-refresh 1s`, a size added to the source is listed under its canonical name and served
/// without a restart, for a URL source (each later GET carrying `If-None-Match` with the stored `ETag`, a 304 keeping
/// the cached document) and for a file source. Served: a request naming it in any spelling is resolved by the
/// refreshed index, no manifest asked, and pulls it; offline, that pull reads the hub cache, which holds no
/// `hf.co/test/` repository.
#[test]
fn serve_refreshes_the_library() -> Result<()> {
    let ardana = Ardana::new("serve-library-refresh-url")?;
    let library = LibraryServer::start(&common::snapshot_bytes()?)?;
    let server = Served::start(
        &ardana,
        &["--library-refresh", "1s"],
        &[("ARDANA_LIBRARY", &library.url)],
    )?;
    let before = common::snapshot_names()?;
    assert_eq!(names(&server.models()?), before);
    let first_etag = library.etag();

    // The next GET revalidates the cached index and is answered 304: the listing stands.
    eventually("the index is read again", REFRESHED, || {
        Ok(library.index_requests().len() >= 2)
    })?;
    let revalidated = &library.index_requests()[1];
    assert_eq!(
        revalidated.if_none_match.as_deref(),
        Some(first_etag.as_str())
    );
    assert_eq!(names(&server.models()?), before);
    assert_eq!(
        std::fs::read_to_string(ardana.home.join("library/models.json.etag"))?,
        first_etag
    );

    // A size added to the published document is listed under its canonical name after its family's others, and the
    // cache moves to the new document and `ETag`.
    let added = common::snapshot_with("9b")?;
    library.publish(&added);
    let mut after = before.clone();
    let at = after
        .iter()
        .position(|n| n == "decider:4b")
        .context("decider:4b")?
        + 1;
    after.insert(at, "decider:9b".into());
    eventually("the added model is listed", REFRESHED, || {
        Ok(names(&server.models()?) == after)
    })?;
    let listed = server.models()?;
    let model = listed["models"]
        .as_array()
        .context("models")?
        .iter()
        .find(|m| m["name"] == "decider:9b")
        .context("decider:9b")?;
    assert_eq!(
        model["description"],
        "hf.co/test/decider-9b-GGUF:decider-2b-v11-Q4_K_M.gguf"
    );
    assert_eq!(model["x_pulled"], false);
    let (status, body) =
        server.decide(&json!({"model": "Decider:9B", "state": "s", "questions": {}}))?;
    assert_eq!(status, 500, "{body}");
    let message = body["detail"]["message"].as_str().unwrap_or_default();
    assert!(
        message.starts_with("pulling decider:9b: ") && message.contains("decider-9b-GGUF"),
        "{body}"
    );
    assert!(
        server.log_text().contains(
            "ardana serve: pulling decider:9b (hf.co/test/decider-9b-GGUF:decider-2b-v11-Q4_K_M.gguf, 1.3 GB)"
        ),
        "{}",
        server.log_text()
    );
    assert!(
        library.requests().iter().all(|r| r.path == "/models.json"),
        "no manifest asked: {:?}",
        library.requests()
    );
    assert_eq!(
        std::fs::read(ardana.home.join("library/models.json"))?,
        added
    );
    assert_eq!(
        std::fs::read_to_string(ardana.home.join("library/models.json.etag"))?,
        library.etag()
    );
    let requests = library.index_requests();
    assert!(
        requests[1..].iter().all(|r| r.if_none_match.is_some()),
        "every later GET revalidates: {requests:?}"
    );
    eventually("the new ETag is revalidated", REFRESHED, || {
        Ok(library
            .index_requests()
            .last()
            .is_some_and(|r| r.if_none_match.as_deref() == Some(library.etag().as_str())))
    })?;
    let log = server.log_text();
    assert!(
        log.contains(&format!(
            "ardana serve: the library at {} lists {} models",
            library.url,
            after.len()
        )),
        "{log}"
    );
    drop(server);

    // A file source is read again the same way, without a cache.
    let ardana = Ardana::new("serve-library-refresh-file")?;
    let document = ardana
        .home
        .parent()
        .context("the home has a parent")?
        .join("models.json");
    std::fs::write(&document, common::snapshot_bytes()?)?;
    let server = Served::start(
        &ardana,
        &["--library-refresh", "1s"],
        &[("ARDANA_LIBRARY", &document.to_string_lossy())],
    )?;
    assert_eq!(names(&server.models()?), before);
    let tmp = document.with_extension("json.tmp");
    std::fs::write(&tmp, common::snapshot_with("9b")?)?;
    std::fs::rename(&tmp, &document)?;
    eventually("the added model is listed from the file", REFRESHED, || {
        Ok(names(&server.models()?) == after)
    })?;
    let (status, body) =
        server.decide(&json!({"model": "decider:9b-q4_k_m", "state": "s", "questions": {}}))?;
    assert_eq!(status, 500, "{body}");
    assert!(
        server
            .log_text()
            .contains("ardana serve: pulling decider:9b (hf.co/test/decider-9b-GGUF:"),
        "{}",
        server.log_text()
    );
    assert!(
        !ardana.home.join("library").exists(),
        "a file source writes no cache"
    );
    Ok(())
}

/// R4.3: with an unreachable source, `serve` binds within 5 s and answers from the cached document; with no cached
/// document it lists the pulled models alone and logs one warning naming the URL.
#[test]
fn serve_outlives_an_unreachable_library() -> Result<()> {
    let ardana = Ardana::new("serve-library-unreachable-cached")?;
    let cached = ardana.home.join("library/models.json");
    std::fs::create_dir_all(cached.parent().context("library")?)?;
    std::fs::write(&cached, common::snapshot_bytes()?)?;
    let started = Instant::now();
    let server = Served::start(&ardana, &[], &[("ARDANA_LIBRARY", CLOSED)])?;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    let listed = server.models()?;
    assert_eq!(names(&listed), common::snapshot_names()?);
    assert!(
        listed["models"]
            .as_array()
            .context("models")?
            .iter()
            .all(|m| m["x_pulled"] == false)
    );
    let log = server.log_text();
    let warning = format!("reading the library at {CLOSED}: ");
    assert_eq!(log.matches(&warning).count(), 1, "{log}");
    assert!(log.contains("; serving the library as last read"), "{log}");
    drop(server);

    let ardana = Ardana::new("serve-library-unreachable-empty")?;
    pull_local(&ardana)?;
    let started = Instant::now();
    let server = Served::start(&ardana, &[], &[("ARDANA_LIBRARY", CLOSED)])?;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    let listed = server.models()?;
    assert_eq!(names(&listed), ["local"]);
    assert_eq!(listed["models"][0]["x_pulled"], true);
    assert_eq!(listed["models"][0]["x_default"], true);
    let log = server.log_text();
    assert_eq!(log.matches(&warning).count(), 1, "{log}");
    assert!(log.contains("; serving the pulled models alone"), "{log}");
    assert!(!ardana.home.join("library").exists(), "nothing cached");
    Ok(())
}

/// R4.6: with `ARDANA_LIBRARY=off`, `serve` sends no library request and lists the pulled models alone; with no
/// document at hand a request without a model resolves to the default-model constant.
#[test]
fn serve_without_a_library() -> Result<()> {
    let ardana = Ardana::new("serve-library-off")?;
    let server = Served::start(&ardana, &[], &[("ARDANA_LIBRARY", "off")])?;
    let banner = server.log_text();
    assert!(
        banner.contains(
            "no models pulled yet; `ardana pull <name>` adds one (https://ardana.ai/models/)"
        ),
        "{banner}"
    );
    assert_eq!(names(&server.models()?), [] as [&str; 0]);
    let (status, body) = server.decide(&json!({"state": "s", "questions": {}}))?;
    assert_eq!(status, 404, "{body}");
    assert_eq!(
        body["detail"]["message"],
        "no model named \"decider\"; none pulled yet; the library at https://ardana.ai/models/ is pulled on first use"
    );
    let (status, body) =
        server.decide(&json!({"model": "decider:0.8b", "state": "s", "questions": {}}))?;
    assert_eq!(status, 404, "{body}");
    drop(server);

    pull_local(&ardana)?;
    let server = Served::start(&ardana, &[], &[("ARDANA_LIBRARY", "off")])?;
    let listed = server.models()?;
    assert_eq!(names(&listed), ["local"]);
    assert_eq!(listed["models"][0]["x_pulled"], true);
    assert_eq!(listed["models"][0]["x_default"], true);
    let log = server.log_text();
    assert!(!log.contains("the library"), "{log}");
    assert!(!ardana.home.join("library").exists(), "nothing cached");
    Ok(())
}

/// R4.7: `ardana serve --help` states the library URL, `--library-refresh` with `ARDANA_LIBRARY_REFRESH` and its
/// default, and `ARDANA_LIBRARY=off`.
#[test]
fn serve_help_documents_the_library_request() -> Result<()> {
    let ardana = Ardana::new("serve-help")?;
    let help = ardana.ok(["serve", "--help"])?;
    for wanted in [
        "https://ardana.ai/models.json",
        "--library-refresh",
        "ARDANA_LIBRARY_REFRESH",
        "[default: 1h]",
        "ARDANA_LIBRARY=off",
    ] {
        assert!(help.contains(wanted), "{wanted:?} in:\n{help}");
    }
    Ok(())
}
