//! R2.1: the browser variants of library models behind `GET /v1/browser/<name>/<file>`, `x_browser` in `/v1/models`,
//! and the cross-origin isolation headers (Q13) on every response. `browser_routes` needs no browser files;
//! `browser_files` serves the real ones `cargo xtask onnx convert` builds into `tmp/hf`. R3.1, R3.2: `public_mode`,
//! the routes of `ardana serve --public`, which runs no model.

mod common;

use std::future::poll_fn;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use ardana_api::SystemOneRequest;
use ardana_core::Layout;
use ardana_registry::{LayoutKind, Registry};
use ardana_server::{ApiError, BODY_LIMIT, ModelOptions, Models, router};
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use common::{Fake, get, hf_file, models, scratch, send, with_held_variants};
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
        &["decider-2b", "stock"],
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
            ("decider-2b", Some(1_121_602_009)),
            ("stock", None),
            ("decider-0.8b", Some(467_748_928)),
            ("decider-4b", None),
            ("qwen3.5-0.8b", Some(904_574_185)),
            ("smollm3-3b", None),
        ]
    );

    // Every other name and file is the API's 404, before any pull; other methods are its 405.
    for uri in [
        "/v1/browser",
        "/v1/browser/decider-0.8b",
        "/v1/browser/decider-0.8b/",
        "/v1/browser/decider-0.8b/model.onnx/extra",
        "/v1/browser/decider-4b/model.onnx",
        "/v1/browser/smollm3-3b/profile",
        "/v1/browser/stock/profile",
        "/v1/browser/nope/tokenizer.json",
        "/v1/browser/DECIDER-0.8B/profile",
        "/v1/browser/decider-0.8b:q8_0/profile",
        "/v1/browser/decider-0.8b/README.md",
        "/v1/browser/decider-0.8b/decider_config.json",
        "/v1/browser/decider-0.8b/tokenizer_config.json",
        "/v1/browser/decider-0.8b/chat_template.jinja",
        "/v1/browser/decider-0.8b/model.onnx.datax",
        "/v1/browser/decider-0.8b/Profile",
        "/v1/browser/decider-0.8b/..%2Fdecider-0.8b-GGUF%2Fdecider_config.json",
        "/v1/browser/..%2F..%2F..%2Fmodels.toml/profile",
        "/v1/browser/decider-0.8b/%2e%2e",
        "/v1/browser/decider-0.8b/%FF",
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
            "/v1/browser/decider-0.8b/model.onnx",
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
        ("/v1/browser/decider-0.8b/profile", 403),
        ("/v1/browser/nope/profile", 403),
        ("/health", 200),
    ] {
        let (got, headers, _) = raw(&keyed, "GET", uri, &[]).await?;
        assert_eq!(got.as_u16(), status, "keyed {uri}");
        isolated(&format!("keyed {uri}"), &headers);
    }
    Ok(())
}

/// R3.2: an error body that names no filesystem path and no upstream URL.
fn tells_no_path(what: &str, body: &str) {
    for told in ["/", "\\", "hf.co", "huggingface"] {
        assert!(!body.contains(told), "{what}: {body} names {told:?}");
    }
}

/// R3.1, R3.2: a public server lists the library alone, every model not pulled, with the browser variants and the
/// browser default and without a default model; refuses every decision with TypeSafe's 403 before reading its body;
/// names no model in `/health`; answers traversal, unknown names, other files and other methods with the API's 404
/// and 405, whose bodies name no path or URL; sends no CORS header, even when asked; and neither loads a runtime nor
/// writes to its empty Ardana home. Nothing here pulls a browser variant (the e2e `public` suite serves them).
#[tokio::test]
async fn public_mode() -> Result<()> {
    let home = scratch("public-mode")?.join("home");
    std::fs::create_dir_all(&home)?;
    let fake = Fake::new();
    let opts = ModelOptions {
        public: true,
        // A public server answers no request with a model: a default model is not looked up, nor listed.
        default_model: Some("decider-4b".into()),
        ..ModelOptions::default()
    };
    let models = Arc::new(Models::new(Registry::open(&home)?, fake.runtimes(), opts)?);
    let app = router(models.clone(), None);
    let asking = [("origin", "https://elsewhere.example")];

    let listed = send(&app, "GET", "/v1/models", &asking, Vec::new()).await?;
    isolated("/v1/models", &listed.headers);
    assert_eq!(
        listed.body,
        with_held_variants(json!({"models": [
            {"name": "decider-2b", "description": "hf.co/Mapika/decider-2b-GGUF:Q4_K_M", "release_date": "2026-09-24",
             "x_pulled": false, "x_size": 1_274_396_800_u64, "x_browser": 1_121_602_009},
            {"name": "decider-0.8b", "description": "hf.co/ardana-ai/decider-0.8b-GGUF:Q8_0",
             "release_date": "2026-09-19", "x_pulled": false, "x_size": 811_843_552, "x_browser": 467_748_928,
             "x_browser_default": true},
            {"name": "decider-4b", "description": "hf.co/Mapika/decider-4b-GGUF:Q4_K_M", "release_date": "2026-09-24",
             "x_pulled": false, "x_size": 2_708_804_640_u64},
            {"name": "qwen3.5-0.8b", "description": "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0",
             "release_date": "2026-02-28", "x_pulled": false, "x_size": 563_036_064, "x_browser": 904_574_185},
            {"name": "smollm3-3b", "description": "hf.co/ggml-org/SmolLM3-3B-GGUF:Q4_K_M",
             "release_date": "2025-07-08", "x_pulled": false, "x_size": 1_915_305_312},
        ]}))
        .await?
    );

    // Every decision is refused, whatever its model or body; the body is never read.
    let ticket = |model: Option<&str>| -> Result<Vec<u8>> {
        let mut body = json!({"state": "My card was charged twice.", "questions": {
            "refund": {"type": "noul", "instructions": "Does the customer ask for a refund?"}}});
        if let Some(model) = model {
            body["model"] = json!(model);
        }
        Ok(serde_json::to_vec(&body)?)
    };
    let json_type = "application/json";
    let refused: [(&str, &str, Vec<u8>); 8] = [
        ("a library model", json_type, ticket(Some("decider-2b"))?),
        (
            "the browser default",
            json_type,
            ticket(Some("decider-0.8b"))?,
        ),
        ("jev-latest", json_type, ticket(Some("jev-latest"))?),
        ("no model", json_type, ticket(None)?),
        ("not JSON", json_type, b"{\"state\": ".to_vec()),
        ("no body", json_type, Vec::new()),
        ("text/plain", "text/plain", ticket(None)?),
        ("over the body limit", json_type, vec![b' '; BODY_LIMIT + 1]),
    ];
    for (label, content_type, body) in refused {
        let headers = [asking[0], ("content-type", content_type)];
        let reply = send(&app, "POST", "/v1/systemone", &headers, body).await?;
        assert_eq!(reply.status, 403, "{label}: {reply:?}");
        let detail = &reply.body["detail"];
        assert_eq!(detail["error_type"], "permission_error", "{label}");
        assert_eq!(detail.as_object().map(|d| d.len()), Some(2), "{label}");
        assert!(
            detail["message"]
                .as_str()
                .is_some_and(|m| m.contains("runs no model") && m.contains("`ardana run`")),
            "{label}: {detail}"
        );
        tells_no_path(label, &reply.body.to_string());
        isolated(label, &reply.headers);
    }
    let request: SystemOneRequest = serde_json::from_slice(&ticket(Some("decider-2b"))?)?;
    assert!(matches!(
        models.decide(request).await,
        Err(ApiError::RunsNoModel)
    ));

    let health = send(&app, "GET", "/health", &asking, Vec::new()).await?;
    assert_eq!(
        (health.status.as_u16(), &health.body),
        (200, &json!({"status": "ok"}))
    );
    isolated("/health", &health.headers);

    // Traversal as sent and percent-encoded, unknown names and other files: the API's 404, before anything is pulled.
    for uri in [
        "/v1/browser/../../models.toml",
        "/v1/browser/../profile",
        "/v1/browser/decider-0.8b/../profile",
        "/v1/browser/decider-0.8b/..",
        "/v1/browser/%2e%2e/profile",
        "/v1/browser/..%2F..%2Fmodels.toml/profile",
        "/v1/browser/decider-0.8b/%2e%2e%2f%2e%2e%2fmodels.toml",
        "/v1/browser/decider-0.8b/..%2Fdecider-0.8b-GGUF%2Fdecider_config.json",
        "/v1/browser/decider-4b/profile",
        "/v1/browser/smollm3-3b/model.onnx",
        "/v1/browser/nope/tokenizer.json",
        "/v1/browser/DECIDER-0.8B/profile",
        "/v1/browser/decider-0.8b:q8_0/profile",
        "/v1/browser/decider-0.8b/README.md",
        "/v1/browser/decider-0.8b/decider_config.json",
        "/v1/browser/decider-0.8b/tokenizer_config.json",
        "/v1/browser/decider-0.8b/chat_template.jinja",
        "/v1/browser/decider-0.8b/model.onnx.datax",
        "/v1/browser/decider-0.8b/%FF",
        "/v1/models/decider-2b",
        "/v1/nope",
    ] {
        let reply = send(&app, "GET", uri, &asking, Vec::new()).await?;
        assert_eq!(
            (reply.status.as_u16(), &reply.body),
            (404, &json!({"detail": "Not Found"})),
            "{uri}"
        );
        tells_no_path(uri, &reply.body.to_string());
        isolated(uri, &reply.headers);
    }
    // Other methods, a CORS preflight among them: the API's 405.
    for (method, uri) in [
        ("POST", "/v1/browser/decider-0.8b/model.onnx"),
        ("PUT", "/v1/browser/decider-0.8b/profile"),
        ("DELETE", "/v1/browser/decider-0.8b/tokenizer.json"),
        ("PATCH", "/v1/browser/decider-0.8b/model.onnx.data"),
        ("POST", "/v1/models"),
        ("OPTIONS", "/v1/models"),
        ("GET", "/v1/systemone"),
        ("PUT", "/v1/systemone"),
    ] {
        let headers = [asking[0], ("access-control-request-method", "POST")];
        let reply = send(&app, method, uri, &headers, Vec::new()).await?;
        assert_eq!(
            (reply.status.as_u16(), &reply.body),
            (405, &json!({"detail": "Method Not Allowed"})),
            "{method} {uri}"
        );
        isolated(&format!("{method} {uri}"), &reply.headers);
    }
    let (status, headers, body) = raw(&app, "POST", "/health", &asking).await?;
    assert_eq!(status.as_u16(), 405);
    tells_no_path("POST /health", &String::from_utf8_lossy(&body));
    isolated("POST /health", &headers);

    assert_eq!(fake.loads(), 0, "a public server loads no runtime");
    let left: Vec<PathBuf> = std::fs::read_dir(&home)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    assert!(left.is_empty(), "the Ardana home stays empty: {left:?}");
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
    let (_, models) = models("browser-files", &["decider-2b"], ModelOptions::default())?;
    let app = router(models.clone(), None);

    // Requests for one model share one pull, and the variant it pulled serves every later request.
    let pulls: Vec<_> = (0..8)
        .map(|_| {
            let models = models.clone();
            tokio::spawn(async move { models.browser("decider-0.8b").await })
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
    let commit = std::fs::read_to_string(
        browser_file(repo, "model.onnx")?
            .ancestors()
            .nth(3)
            .context("the repository directory")?
            .join("refs/main"),
    )?;
    assert_eq!(model.commit, commit.trim());
    assert_eq!(model.reference, format!("hf.co/{repo}"));

    // The files, from the hub cache, as stored: never compressed, with their length, the commit as their ETag.
    let mut total = 0;
    for (file, content_type) in [
        ("model.onnx", "application/octet-stream"),
        ("model.onnx.data", "application/octet-stream"),
        ("tokenizer.json", "application/json"),
    ] {
        let uri = format!("/v1/browser/decider-0.8b/{file}");
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
        let uri = format!("/v1/browser/decider-0.8b/{file}");
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
        "/v1/browser/decider-0.8b/profile",
        &[("range", "bytes=0-15")],
    )
    .await?;
    assert_eq!(status, 200);
    assert_eq!(headers.get("accept-ranges"), None);
    assert!(serde_json::from_slice::<Value>(&body)?.is_object());
    let listed = get(&app, "/v1/models").await?;
    let listed = listed.body["models"]
        .as_array()
        .and_then(|models| models.iter().find(|m| m["name"] == "decider-0.8b"))
        .context("decider-0.8b is listed")?
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

    // The profile, as `ardana pull decider-0.8b` derives it from the same decider_config.json.
    let profile = get(&app, "/v1/browser/decider-0.8b/profile").await?;
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

    // qwen3.5-0.8b has no decider_config.json: the stock profile in the chat layout of the chat template beside its
    // tokenizer, which equals the one the native model reads from Qwen/Qwen3.5-0.8B.
    let qwen = get(&app, "/v1/browser/qwen3.5-0.8b/profile").await?;
    assert_eq!(qwen.status, 200, "{qwen:?}");
    let native_tokenizer = hf_file("Qwen/Qwen3.5-0.8B", "tokenizer.json")?;
    let mut native = ardana_registry::read_profile(
        &native_tokenizer,
        &ardana_registry::load_tokenizer(&native_tokenizer)?,
        None,
        LayoutKind::Chat,
        "qwen3.5-0.8b",
    )?;
    native.release_date = Some("2026-02-28".into());
    assert_eq!(qwen.body, serde_json::to_value(&native)?);
    assert!(
        matches!(native.layout, Layout::Chat { .. }),
        "{:?}",
        native.layout
    );
    Ok(())
}
