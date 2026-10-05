//! R2.1: the browser variants of library models behind `GET /v1/browser/<name>/<file>`, `x_browser` in `/v1/models`,
//! and the cross-origin isolation headers (Q13) on every response; R3.4: a variant is read at the commit its library
//! entry names. `browser_routes` needs no browser files; `browser_files` serves the real ones `cargo xtask onnx
//! convert` builds into `tmp/hf`.

mod common;

use std::future::poll_fn;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use ardana_core::Layout;
use ardana_registry::LayoutKind;
use ardana_registry::library::{LibraryDocument, LibrarySource};
use ardana_server::{ModelOptions, router};
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use common::{get, hf_file, models, send, with_held_variants};
use http_body::Body as _;
use serde_json::{Value, json};
use tower::ServiceExt;

/// Q13: the isolation headers, and no CORS header.
fn isolated(what: &str, headers: &HeaderMap) {
    for (name, value) in [
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-embedder-policy", "require-corp"),
        ("cross-origin-resource-policy", "same-origin"),
    ] {
        assert_eq!(
            headers.get(name).and_then(|v| v.to_str().ok()),
            Some(value),
            "{what}: {name}"
        );
    }
    let cors: Vec<&str> = headers
        .keys()
        .map(|name| name.as_str())
        .filter(|name| name.starts_with("access-control-"))
        .collect();
    assert!(cors.is_empty(), "{what}: {cors:?}");
}

/// `method uri` with `headers`: the status, the headers and the body, whatever its type.
async fn raw(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> Result<(StatusCode, HeaderMap, Vec<u8>)> {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app.clone().oneshot(request.body(Body::empty())?).await?;
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await?;
    Ok((parts.status, parts.headers, bytes.to_vec()))
}

#[tokio::test]
async fn browser_routes() -> Result<()> {
    let (_, models) = models(
        "browser-routes",
        &["decider:2b", "stock"],
        ModelOptions::default(),
    )?;
    let app = router(models.clone(), None);

    // `x_browser` marks the three models with a browser variant, pulled or not, with the bytes a tab downloads, and
    // `x_browser_pulled` those the hub cache holds whole.
    let listed = get(&app, "/v1/models").await?;
    isolated("/v1/models", &listed.headers);
    let mut unmarked = listed.body.clone();
    for model in unmarked["models"].as_array_mut().context("models")? {
        model
            .as_object_mut()
            .context("a model")?
            .remove("x_browser_pulled");
    }
    assert_eq!(listed.body, with_held_variants(unmarked).await?);
    let browser: Vec<(&str, Option<u64>)> = listed.body["models"]
        .as_array()
        .context("models")?
        .iter()
        .map(|m| {
            (
                m["name"].as_str().unwrap_or_default(),
                m["x_browser"].as_u64(),
            )
        })
        .collect();
    assert_eq!(
        browser,
        [
            ("decider:2b", Some(1_121_602_009)),
            ("stock", None),
            ("decider:0.8b", Some(467_748_928)),
            ("decider:4b", None),
            ("gemma-4:e2b", None),
            ("gemma-4:e4b", None),
            ("gemma-4:12b", None),
            ("gemma-4:26b-a4b", None),
            ("gemma-4:31b", None),
            ("qwen3.5:0.8b", Some(904_574_185)),
            ("qwen3.6:35b-a3b", None),
            ("qwen3.8:27b", None),
            ("smollm3:3b", None),
        ]
    );

    // R3.4: the cache holds a variant only at the commit the library names; a document naming one it lacks lists every
    // variant as not pulled.
    let mut moved = common::library()?;
    for size in moved.models.iter_mut().flat_map(|family| &mut family.sizes) {
        if let Some(browser) = &mut size.browser {
            browser.commit = "1111111111111111111111111111111111111111".into();
        }
    }
    let document = common::scratch("browser-routes-moved-library")?.join("models.json");
    std::fs::write(
        &document,
        serde_json::to_vec(&LibraryDocument {
            schema: 1,
            default: moved.default,
            browser_default: moved.browser_default,
            models: moved.models,
        })?,
    )?;
    let (_, elsewhere) = common::models_from(
        "browser-routes-moved",
        &["decider:2b"],
        ModelOptions::default(),
        LibrarySource::File(document),
    )?;
    let listed = get(&router(elsewhere, None), "/v1/models").await?;
    let pulled: Vec<&Value> = listed.body["models"]
        .as_array()
        .context("models")?
        .iter()
        .filter(|m| m.get("x_browser_pulled").is_some())
        .collect();
    assert!(pulled.is_empty(), "{pulled:?}");

    // Every other name and file is the API's 404, before any pull: a size's canonical name is looked up whole (Q19), so
    // a family, another spelling and a size without a browser variant name nothing; other methods are its 405.
    for uri in [
        "/v1/browser",
        "/v1/browser/decider:0.8b",
        "/v1/browser/decider:0.8b/",
        "/v1/browser/decider:0.8b/model.onnx/extra",
        "/v1/browser/decider/model.onnx",
        "/v1/browser/decider:latest/model.onnx",
        "/v1/browser/decider:4b/model.onnx",
        "/v1/browser/gemma-4:e2b/model.onnx",
        "/v1/browser/smollm3:3b/profile",
        "/v1/browser/stock/profile",
        "/v1/browser/nope/tokenizer.json",
        "/v1/browser/DECIDER:0.8B/profile",
        "/v1/browser/decider:0.8b-q8_0/profile",
        "/v1/browser/decider:0.8b/README.md",
        "/v1/browser/decider:0.8b/decider_config.json",
        "/v1/browser/decider:0.8b/tokenizer_config.json",
        "/v1/browser/decider:0.8b/chat_template.jinja",
        "/v1/browser/decider:0.8b/model.onnx.datax",
        "/v1/browser/decider:0.8b/Profile",
        "/v1/browser/decider:0.8b/..%2Fdecider-0.8b-GGUF%2Fdecider_config.json",
        "/v1/browser/..%2F..%2F..%2Fmodels.toml/profile",
        "/v1/browser/decider:0.8b/%2e%2e",
        "/v1/browser/decider:0.8b/%FF",
    ] {
        let reply = get(&app, uri).await?;
        assert_eq!(
            (reply.status.as_u16(), &reply.body),
            (404, &json!({"detail": "Not Found"})),
            "{uri}"
        );
        isolated(uri, &reply.headers);
    }
    for method in ["POST", "PUT", "DELETE"] {
        let reply = send(
            &app,
            method,
            "/v1/browser/decider:0.8b/model.onnx",
            &[],
            Vec::new(),
        )
        .await?;
        assert_eq!(
            (reply.status.as_u16(), &reply.body),
            (405, &json!({"detail": "Method Not Allowed"})),
            "{method}"
        );
        isolated(method, &reply.headers);
    }

    // Every other response is isolated too: the playground, its SPA fallback, /health, the API and its errors.
    for (method, uri) in [
        ("GET", "/"),
        ("GET", "/index.html"),
        ("GET", "/some/deep/link"),
        ("GET", "/health"),
        ("GET", "/v1/nope"),
        ("POST", "/v1/systemone"),
        ("GET", "/v1/systemone"),
    ] {
        let (status, headers, _) = raw(&app, method, uri, &[("accept-encoding", "br")]).await?;
        assert!(status.as_u16() < 500, "{method} {uri}: {status}");
        isolated(&format!("{method} {uri}"), &headers);
    }
    let keyed = router(models, Some("secret".into()));
    for (uri, status) in [
        ("/v1/models", 403),
        ("/v1/browser/decider:0.8b/profile", 403),
        ("/v1/browser/nope/profile", 403),
        ("/health", 200),
    ] {
        let (got, headers, _) = raw(&keyed, "GET", uri, &[]).await?;
        assert_eq!(got.as_u16(), status, "keyed {uri}");
        isolated(&format!("keyed {uri}"), &headers);
    }
    Ok(())
}

/// The cache path of a file of the browser variant `repo` (`ardana-ai/<name>-ONNX`), which `cargo xtask onnx convert`
/// writes into `tmp/hf`.
fn browser_file(repo: &str, name: &str) -> Result<PathBuf> {
    hf_file(repo, name).with_context(|| format!("{repo}: run `cargo xtask onnx convert` for it"))
}

/// Reads `body` frame by frame and checks it equals the file at `path`, never holding more than a frame.
async fn same_bytes(body: Body, path: &Path) -> Result<u64> {
    let mut file = std::fs::File::open(path)?;
    let mut body = body;
    let mut seen = 0u64;
    let mut expected = Vec::new();
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let Ok(data) = frame?.into_data() else {
            continue;
        };
        expected.resize(data.len(), 0);
        file.read_exact(&mut expected)
            .with_context(|| format!("the body is longer than {}", path.display()))?;
        ensure!(
            data[..] == expected[..],
            "the body differs from {} after byte {seen}",
            path.display()
        );
        seen += data.len() as u64;
    }
    ensure!(
        file.read(&mut [0u8; 1])? == 0,
        "the body is shorter than {}",
        path.display()
    );
    Ok(seen)
}

#[tokio::test]
#[ignore = "e2e: the browser variants `cargo xtask onnx convert` builds into tmp/hf"]
async fn browser_files() -> Result<()> {
    let repo = "ardana-ai/decider-0.8b-ONNX";
    let (_, models) = models("browser-files", &["decider:2b"], ModelOptions::default())?;
    let app = router(models.clone(), None);

    // Requests for one model share one pull, and the variant it pulled serves every later request.
    let pulls: Vec<_> = (0..8)
        .map(|_| {
            let models = models.clone();
            tokio::spawn(async move { models.browser("decider:0.8b").await })
        })
        .collect();
    let mut pulled = Vec::new();
    for pull in pulls {
        pulled.push(pull.await?.map_err(|err| anyhow::anyhow!("{err:?}"))?);
    }
    ensure!(
        pulled.iter().all(|model| Arc::ptr_eq(model, &pulled[0])),
        "every request shares one pull"
    );
    let model = pulled[0].clone();
    // R3.4: the files of the commit the library entry names.
    let library = common::library()?;
    let pick = library
        .browser("decider:0.8b")
        .context("decider:0.8b has a browser variant")?;
    let browser = pick.size.browser.context("a browser variant")?;
    assert_eq!(model.commit, browser.commit);
    assert_eq!(model.reference, format!("hf.co/{repo}"));
    let snapshot = PathBuf::from(std::env::var_os("HF_HOME").context("HF_HOME is not set")?)
        .join("hub/models--ardana-ai--decider-0.8b-ONNX/snapshots")
        .join(&browser.commit);
    let files: Vec<PathBuf> = ardana_registry::BROWSER_FILES
        .iter()
        .map(|file| snapshot.join(file))
        .collect();
    assert_eq!(model.files, files);

    // The files, from the hub cache, as stored: never compressed, with their length, the commit as their ETag.
    let mut total = 0;
    for (file, content_type) in [
        ("model.onnx", "application/octet-stream"),
        ("model.onnx.data", "application/octet-stream"),
        ("tokenizer.json", "application/json"),
    ] {
        let uri = format!("/v1/browser/decider:0.8b/{file}");
        let request = Request::builder()
            .uri(&uri)
            .header("accept-encoding", "br, gzip, deflate")
            .body(Body::empty())?;
        let response = app.clone().oneshot(request).await?;
        let path = browser_file(repo, file)?;
        let size = std::fs::metadata(&path)?.len();
        let headers = response.headers().clone();
        assert_eq!(response.status(), 200, "{uri}");
        isolated(&uri, &headers);
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        };
        assert_eq!(header("content-encoding"), None, "{uri}");
        assert_eq!(header("content-length"), Some(size.to_string()), "{uri}");
        assert_eq!(
            header("content-type").as_deref(),
            Some(content_type),
            "{uri}"
        );
        assert_eq!(
            header("etag"),
            Some(format!("\"{}\"", model.commit)),
            "{uri}"
        );
        assert_eq!(
            header("cache-control").as_deref(),
            Some("no-store"),
            "{uri}"
        );
        assert_eq!(header("accept-ranges").as_deref(), Some("bytes"), "{uri}");
        assert_eq!(
            same_bytes(response.into_body(), &path).await?,
            size,
            "{uri}"
        );
        total += size;
    }

    // Byte ranges (RFC 9110), which a tab resumes a download with: one range is its part (206, `Content-Range`, the
    // part's length), of this version (`If-Range`) or none named; another version, several ranges or one that does
    // not parse are the whole file; a range past the end is a 416 that names the length. The profile is always whole.
    let version = format!("\"{}\"", model.commit);
    for file in ["model.onnx", "model.onnx.data"] {
        let uri = format!("/v1/browser/decider:0.8b/{file}");
        let path = browser_file(repo, file)?;
        let len = std::fs::metadata(&path)?.len();
        let slice = |start: u64, end: u64| -> Result<Vec<u8>> {
            let mut read = std::fs::File::open(&path)?;
            std::io::Seek::seek(&mut read, std::io::SeekFrom::Start(start))?;
            let mut bytes = vec![0; usize::try_from(end - start + 1)?];
            read.read_exact(&mut bytes)?;
            Ok(bytes)
        };
        let middle = len / 2;
        for (range, if_range, start, end) in [
            ("bytes=0-15".to_string(), None, 0, 15),
            (
                format!("bytes={middle}-"),
                Some(version.as_str()),
                middle,
                len - 1,
            ),
            (
                format!("bytes={middle}-{}", middle + 99),
                None,
                middle,
                middle + 99,
            ),
            ("bytes=-10".to_string(), None, len - 10, len - 1),
            (
                format!("bytes={}-{}", len - 3, len + 50),
                None,
                len - 3,
                len - 1,
            ),
        ] {
            let mut sent = vec![("range", range.as_str()), ("accept-encoding", "br")];
            sent.extend(if_range.map(|version| ("if-range", version)));
            let (status, headers, body) = raw(&app, "GET", &uri, &sent).await?;
            let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
            assert_eq!(status, 206, "{uri} {range}");
            isolated(&format!("{uri} {range}"), &headers);
            assert_eq!(
                header("content-range"),
                Some(format!("bytes {start}-{end}/{len}").as_str()),
                "{uri} {range}"
            );
            assert_eq!(
                header("content-length"),
                Some((end - start + 1).to_string().as_str()),
                "{uri} {range}"
            );
            assert_eq!(header("accept-ranges"), Some("bytes"), "{uri} {range}");
            assert_eq!(header("etag"), Some(version.as_str()), "{uri} {range}");
            assert_eq!(header("cache-control"), Some("no-store"), "{uri} {range}");
            assert_eq!(header("content-encoding"), None, "{uri} {range}");
            assert!(body == slice(start, end)?, "{uri} {range}: other bytes");
        }
        for (range, if_range) in [
            ("bytes=0-15", Some("\"another commit\"")),
            ("bytes=0-15", Some("W/\"another commit\"")),
            ("bytes=0-1,4-5", None),
            ("bytes=9-1", None),
        ] {
            let mut sent = vec![("range", range)];
            sent.extend(if_range.map(|version| ("if-range", version)));
            let response = app
                .clone()
                .oneshot(
                    sent.iter()
                        .fold(Request::builder().uri(&uri), |request, (name, value)| {
                            request.header(*name, *value)
                        })
                        .body(Body::empty())?,
                )
                .await?;
            let headers = response.headers().clone();
            assert_eq!(response.status(), 200, "{uri} {range} {if_range:?}");
            assert_eq!(
                headers.get("content-length").and_then(|v| v.to_str().ok()),
                Some(len.to_string().as_str()),
                "{uri} {range} {if_range:?}"
            );
            assert_eq!(
                same_bytes(response.into_body(), &path).await?,
                len,
                "{uri} {range}"
            );
        }
        let past = format!("bytes={len}-");
        let (status, headers, body) = raw(&app, "GET", &uri, &[("range", &past)]).await?;
        assert_eq!(status, 416, "{uri} {past}");
        isolated(&format!("{uri} {past}"), &headers);
        assert_eq!(
            headers.get("content-range").and_then(|v| v.to_str().ok()),
            Some(format!("bytes */{len}").as_str())
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&body)?,
            json!({"detail": "Range Not Satisfiable"})
        );
    }
    let (status, headers, body) = raw(
        &app,
        "GET",
        "/v1/browser/decider:0.8b/profile",
        &[("range", "bytes=0-15")],
    )
    .await?;
    assert_eq!(status, 200);
    assert_eq!(headers.get("accept-ranges"), None);
    assert!(serde_json::from_slice::<Value>(&body)?.is_object());
    let listed = get(&app, "/v1/models").await?;
    let listed = listed.body["models"]
        .as_array()
        .and_then(|models| models.iter().find(|m| m["name"] == "decider:0.8b"))
        .context("decider:0.8b is listed")?
        .clone();
    assert_eq!(
        listed["x_browser"],
        json!(total),
        "x_browser is what a tab downloads"
    );
    assert_eq!(
        listed["x_browser_pulled"], true,
        "the hub cache holds the variant whole: a request for its files pulls nothing"
    );

    // The profile, as `ardana pull decider:0.8b` derives it from the same decider_config.json.
    let profile = get(&app, "/v1/browser/decider:0.8b/profile").await?;
    isolated("profile", &profile.headers);
    assert_eq!(profile.status, 200);
    assert_eq!(
        profile.headers.get("etag").and_then(|v| v.to_str().ok()),
        Some(format!("\"{}\"", model.commit).as_str())
    );
    let config: Value = serde_json::from_str(&std::fs::read_to_string(browser_file(
        repo,
        "decider_config.json",
    )?)?)?;
    let mut expected = ardana_core::from_decider_config(&config, Layout::Plain)?;
    expected.release_date = Some("2026-09-19".into());
    assert_eq!(profile.body, serde_json::to_value(&expected)?);
    assert_eq!(profile.body["name"], "decider-0.8b-v1");
    assert_eq!(profile.body["layout"], json!({"kind": "plain"}));

    // qwen3.5:0.8b has no decider_config.json: the stock profile, named canonically, in the chat layout of the chat
    // template beside its tokenizer, which equals the one the native model reads from Qwen/Qwen3.5-0.8B.
    let qwen = get(&app, "/v1/browser/qwen3.5:0.8b/profile").await?;
    assert_eq!(qwen.status, 200, "{qwen:?}");
    let native_tokenizer = hf_file("Qwen/Qwen3.5-0.8B", "tokenizer.json")?;
    let mut native = ardana_registry::read_profile(
        &native_tokenizer,
        &ardana_registry::load_tokenizer(&native_tokenizer)?,
        None,
        LayoutKind::Chat,
        "qwen3.5:0.8b",
    )?;
    native.release_date = Some("2026-02-28".into());
    assert_eq!(qwen.body, serde_json::to_value(&native)?);
    assert_eq!(qwen.body["name"], "qwen3.5:0.8b");
    assert!(
        matches!(native.layout, Layout::Chat { .. }),
        "{:?}",
        native.layout
    );
    Ok(())
}
