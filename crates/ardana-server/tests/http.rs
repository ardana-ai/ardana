//! R5.4 to R5.6 and R5.8 through the router: the API surface, model resolution, every error shape, and the
//! `ardana serve` defaults; R4.4: a name the library's index lacks is asked of the library once. Requests are read
//! with decider-2b's real tokenizer and profile; the weights are the fake runtime's, since these tests check what
//! happens around a decode (the real model runs in `cargo xtask e2e jevcompat|sdk|jevbench`).

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use ardana_registry::library::LibrarySource;
use ardana_server::{ModelOptions, ServeArgs, router};
use axum::extract::{Path, Request};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::get as route_get;
use axum::{Json, Router};
use clap::{CommandFactory, Parser};
use common::{Reply, eventually, get, models, post, send, with_held_variants};
use serde_json::{Value, json};

fn noul(instructions: &str) -> Value {
    json!({"type": "noul", "instructions": instructions})
}

/// A body of `n` repetitions of a one-token word, as a state.
fn words(n: usize) -> String {
    " word".repeat(n)
}

/// `{"detail": {"error_type": <error_type>, "message": str}}`; returns the message.
fn error_shape(reply: &Reply, error_type: &str) -> String {
    let detail = &reply.body["detail"];
    assert_eq!(detail["error_type"], error_type, "{reply:?}");
    assert_eq!(detail.as_object().map(|d| d.len()), Some(2), "{reply:?}");
    detail["message"]
        .as_str()
        .unwrap_or_else(|| panic!("no message: {reply:?}"))
        .to_string()
}

/// A 422 `{"detail": [{"loc": ["body", ..], "msg": str, "type": str}, ..]}`.
fn validation_shape(reply: &Reply) {
    assert_eq!(reply.status, 422, "{reply:?}");
    let items = reply.body["detail"]
        .as_array()
        .unwrap_or_else(|| panic!("{reply:?}"));
    assert!(!items.is_empty(), "{reply:?}");
    for item in items {
        assert_eq!(item["loc"][0], "body", "{reply:?}");
        assert!(
            item["msg"].is_string() && item["type"].is_string(),
            "{reply:?}"
        );
    }
}

#[tokio::test]
async fn surface() -> Result<()> {
    let (fake, models) = models(
        "http-surface",
        &["decider-2b", "stock"],
        ModelOptions::default(),
    )?;
    let app = router(models.clone(), None);

    let health = get(&app, "/health").await?;
    assert_eq!(health.status, 200);
    assert_eq!(health.body["status"], "ok");
    assert_eq!(health.body["x_loaded"], json!([]));
    let temperatures = &health.body["x_temperature_by_type"]["decider-2b"];
    for t in ["choice", "noul", "score"] {
        assert!(
            temperatures[t].as_f64().is_some_and(|v| v > 0.0),
            "{health:?}"
        );
    }

    let listed = get(&app, "/v1/models").await?;
    assert_eq!(listed.status, 200);
    assert_eq!(
        listed.body,
        with_held_variants(json!({"models": [
            {"name": "decider-2b", "description": "hf.co/test/decider-2b-GGUF", "release_date": "2026-09-24",
             "x_pulled": true, "x_default": true, "x_browser": 1_121_602_009},
            {"name": "stock", "description": "hf.co/test/stock-GGUF", "release_date": "2026-09-28", "x_pulled": true},
            {"name": "decider-0.8b", "description": "hf.co/ardana-ai/decider-0.8b-GGUF:Q8_0",
             "release_date": "2026-09-19", "x_pulled": false, "x_size": 811_843_552, "x_browser": 467_748_928,
             "x_browser_default": true},
            {"name": "decider-4b", "description": "hf.co/Mapika/decider-4b-GGUF:Q4_K_M", "release_date": "2026-09-24",
             "x_pulled": false, "x_size": 2_708_804_640_u64},
            {"name": "qwen3.5-0.8b", "description": "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0",
             "release_date": "2026-02-28", "x_pulled": false, "x_size": 563_036_064, "x_browser": 904_574_185},
            {"name": "smollm3-3b", "description": "hf.co/ggml-org/SmolLM3-3B-GGUF:Q4_K_M",
             "release_date": "2025-07-08", "x_pulled": false, "x_size": 1_915_305_312},
            {"name": "gemma-4-e2b", "description": "hf.co/ggml-org/gemma-4-E2B-it-GGUF:Q4_0",
             "release_date": "2026-03-02", "x_pulled": false, "x_size": 2_841_481_184_u64},
            {"name": "gemma-4-e4b", "description": "hf.co/ggml-org/gemma-4-E4B-it-GGUF:Q4_0",
             "release_date": "2026-03-02", "x_pulled": false, "x_size": 4_590_807_392_u64},
            {"name": "gemma-4-12b", "description": "hf.co/ggml-org/gemma-4-12B-it-GGUF:Q4_0",
             "release_date": "2026-05-23", "x_pulled": false, "x_size": 7_219_673_216_u64},
            {"name": "gemma-4-26b-a4b", "description": "hf.co/ggml-org/gemma-4-26B-A4B-it-GGUF:Q4_0",
             "release_date": "2026-03-11", "x_pulled": false, "x_size": 14_618_145_824_u64},
            {"name": "gemma-4-31b", "description": "hf.co/ggml-org/gemma-4-31B-it-GGUF:Q4_0",
             "release_date": "2026-03-11", "x_pulled": false, "x_size": 17_992_313_088_u64},
            {"name": "qwen3.6-35b-a3b", "description": "hf.co/ggml-org/Qwen3.6-35B-A3B-GGUF:Q4_K_M",
             "release_date": "2026-04-15", "x_pulled": false, "x_size": 20_419_565_568_u64},
            {"name": "qwen3.8-27b", "description": "hf.co/ggml-org/Qwen3.8-27B-GGUF:Q4_K_M",
             "release_date": "2026-08-05", "x_pulled": false, "x_size": 18_973_870_528_u64},
        ]}))
        .await?
    );
    assert_eq!(fake.loads(), 0, "listing loads nothing");

    let empty = post(
        &app,
        &json!({"model": "jev-latest", "state": "s", "questions": {}}),
    )
    .await?;
    assert_eq!(empty.status, 200, "{empty:?}");
    assert_eq!(empty.body["model"], "decider-2b");
    assert_eq!(empty.body["answers"], json!({}));
    assert_eq!(empty.body["usage"]["output_tokens"], 0);
    assert_eq!(
        get(&app, "/health").await?.body["x_loaded"],
        json!(["decider-2b"])
    );

    let answered = post(
        &app,
        &json!({"state": "s", "questions": {"q": noul("Is it?")}}),
    )
    .await?;
    assert_eq!(answered.status, 200, "{answered:?}");
    assert_eq!(
        answered.body["answers"]["q"],
        json!({"type": "noul", "noul": 0.5})
    );

    for (method, uri) in [
        ("GET", "/v1/nope"),
        ("POST", "/v1/systemone/extra"),
        ("GET", "/v1"),
    ] {
        let reply = send(&app, method, uri, &[], Vec::new()).await?;
        assert_eq!(
            (reply.status.as_u16(), &reply.body),
            (404, &json!({"detail": "Not Found"})),
            "{method} {uri}"
        );
    }
    let keyed = router(models, Some("secret".into()));
    let reply = get(&keyed, "/v1/nope").await?;
    assert_eq!(
        (reply.status.as_u16(), reply.body),
        (404, json!({"detail": "Not Found"}))
    );
    let reply = get(&app, "/v1/systemone").await?;
    assert_eq!(
        (reply.status.as_u16(), reply.body),
        (405, json!({"detail": "Method Not Allowed"}))
    );
    Ok(())
}

#[tokio::test]
async fn model_resolution() -> Result<()> {
    let (_, models) = models(
        "http-model-resolution",
        &["alpha", "beta"],
        ModelOptions::default(),
    )?;
    let app = router(models, None);
    let ask = |model: Option<&str>| {
        let mut body = json!({"state": "s", "questions": {}});
        if let Some(model) = model {
            body["model"] = json!(model);
        }
        body
    };
    for (model, answered_by) in [
        (None, "alpha"),
        (Some("jev-latest"), "alpha"),
        (Some("jev-1.13.0"), "alpha"),
        (Some("alpha"), "alpha"),
        (Some("beta"), "beta"),
    ] {
        let reply = post(&app, &ask(model)).await?;
        assert_eq!(
            (reply.status.as_u16(), &reply.body["model"]),
            (200, &json!(answered_by)),
            "{model:?}"
        );
    }
    let unknown = post(&app, &ask(Some("gamma"))).await?;
    assert_eq!(unknown.status, 404);
    assert_eq!(
        error_shape(&unknown, "not_found_error"),
        "no model named \"gamma\"; pulled: alpha, beta; the library at https://ardana.ai/models/ is pulled on first use"
    );

    let opts = ModelOptions {
        default_model: Some("beta".into()),
        ..ModelOptions::default()
    };
    let (_, models) = common::models("http-model-resolution-default", &["alpha", "beta"], opts)?;
    let app = router(models, None);
    for model in [None, Some("jev-latest"), Some("jev-anything")] {
        assert_eq!(
            post(&app, &ask(model)).await?.body["model"],
            "beta",
            "{model:?}"
        );
    }

    let opts = ModelOptions {
        default_model: Some("gamma".into()),
        ..ModelOptions::default()
    };
    let err = common::models("http-model-resolution-bad-default", &["alpha"], opts).unwrap_err();
    assert!(
        err.to_string().contains(
            "no model named \"gamma\"; pulled: alpha; the library at https://ardana.ai/models/"
        ),
        "{err}"
    );

    // A library model may be the default before it is pulled; a registry entry of its name is the pulled one.
    let opts = ModelOptions {
        default_model: Some("decider-4b:q4_k_m".into()),
        ..ModelOptions::default()
    };
    let (_, models) = common::models(
        "http-model-resolution-library-default",
        &["decider-2b"],
        opts,
    )?;
    let listed = get(&router(models, None), "/v1/models").await?.body;
    let flags: Vec<(&str, bool, bool)> = listed["models"]
        .as_array()
        .context("models")?
        .iter()
        .map(|m| {
            (
                m["name"].as_str().unwrap_or_default(),
                m["x_pulled"] == true,
                m["x_default"] == true,
            )
        })
        .collect();
    assert_eq!(
        flags,
        [
            ("decider-2b", true, false),
            ("decider-0.8b", false, false),
            ("decider-4b", false, true),
            ("qwen3.5-0.8b", false, false),
            ("smollm3-3b", false, false),
            ("gemma-4-e2b", false, false),
            ("gemma-4-e4b", false, false),
            ("gemma-4-12b", false, false),
            ("gemma-4-26b-a4b", false, false),
            ("gemma-4-31b", false, false),
            ("qwen3.6-35b-a3b", false, false),
            ("qwen3.8-27b", false, false),
        ]
    );

    // With nothing pulled, the library default is the default model and an unknown name points to the library.
    let (_, models) = common::models("http-model-resolution-empty", &[], ModelOptions::default())?;
    let app = router(models, None);
    let listed = get(&app, "/v1/models").await?.body;
    assert_eq!(listed["models"][0]["name"], "decider-2b");
    assert_eq!(listed["models"][0]["x_default"], true);
    assert_eq!(listed["models"][0]["x_size"], 1_274_396_800_u64);
    assert_eq!(
        listed["models"].as_array().map(Vec::len),
        Some(12),
        "{listed}"
    );
    let reply = post(&app, &ask(Some("gamma"))).await?;
    assert_eq!(reply.status, 404);
    assert!(
        error_shape(&reply, "not_found_error")
            == "no model named \"gamma\"; none pulled yet; the library at https://ardana.ai/models/ is pulled on \
                first use",
        "{reply:?}"
    );
    Ok(())
}

/// `models.toml` is reread when it changes: a model added while serving is listed and answers, a removed one is gone,
/// and a changed entry is read with its new profile.
#[tokio::test]
async fn registry_changes_are_picked_up() -> Result<()> {
    let (fake, models) = models("http-registry-changes", &["alpha"], ModelOptions::default())?;
    let app = router(models.clone(), None);
    let ask =
        |model: &str| json!({"model": model, "state": "s", "questions": {"q": noul("Is it?")}});
    assert_eq!(post(&app, &ask("alpha")).await?.status, 200);
    let path = models
        .registry()
        .map_err(|err| anyhow::anyhow!("{err:?}"))?
        .path()
        .to_path_buf();
    let home = path.parent().context("home")?;
    let dir = home.parent().context("the test's scratch directory")?;

    let mut registry = ardana_registry::Registry::open(home)?;
    registry.insert(common::fake_entry(dir, "beta")?);
    registry.save()?;
    let names = |body: &Value| -> Vec<String> {
        body["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["x_pulled"] == true)
            .filter_map(|m| m["name"].as_str().map(String::from))
            .collect()
    };
    assert_eq!(
        names(&get(&app, "/v1/models").await?.body),
        ["alpha", "beta"]
    );
    let beta = post(&app, &ask("beta")).await?;
    assert_eq!(
        (beta.status.as_u16(), &beta.body["model"]),
        (200, &json!("beta"))
    );

    let mut registry = ardana_registry::Registry::open(home)?;
    registry.remove("alpha")?;
    let mut renamed = registry.entry("beta")?.clone();
    renamed.profile.name = "beta-v2".into();
    registry.insert(renamed);
    registry.save()?;
    assert_eq!(names(&get(&app, "/v1/models").await?.body), ["beta"]);
    let gone = post(&app, &ask("alpha")).await?;
    assert_eq!(gone.status, 404, "{gone:?}");
    let changed = post(&app, &ask("beta")).await?;
    assert_eq!(changed.body["model"], "beta-v2", "{changed:?}");
    assert_eq!(fake.loads(), 3, "the changed entry is loaded again");
    Ok(())
}

#[tokio::test]
async fn errors() -> Result<()> {
    let opts = ModelOptions {
        max_queued_rows: 2,
        ..ModelOptions::default()
    };
    let (fake, models) = models("http-errors", &["decider-2b"], opts)?;
    let app = router(models.clone(), None);
    let json_type = [("content-type", "application/json")];

    // 422: every malformed body, in FastAPI's shape.
    let base = json!({"state": "s", "questions": {"q": noul("Is it?")}});
    // (case, content type, body)
    let mut invalid: Vec<(&str, &str, Vec<u8>)> = vec![
        (
            "not JSON",
            "application/json",
            br#"{"state": "s", "questions": {"#.to_vec(),
        ),
        ("empty body", "application/json", Vec::new()),
        ("not an object", "application/json", b"[]".to_vec()),
        ("text/plain", "text/plain", serde_json::to_vec(&base)?),
    ];
    let edits: [(&str, Value); 9] = [
        ("missing state", json!({"questions": {"q": noul("Is it?")}})),
        ("missing questions", json!({"state": "s"})),
        ("questions a list", json!({"state": "s", "questions": []})),
        (
            "model a number",
            json!({"model": 5, "state": "s", "questions": {}}),
        ),
        (
            "missing type",
            json!({"state": "s", "questions": {"q": {"instructions": "Is it?"}}}),
        ),
        (
            "unknown type",
            json!({"state": "s", "questions": {"q": {"type": "boolean", "instructions": "Is it?"}}}),
        ),
        (
            "choice without options",
            json!({"state": "s", "questions": {"q": {"type": "choice", "instructions": "Which?", "criteria": {}}}}),
        ),
        (
            "score with one level",
            json!({"state": "s", "questions": {"q": {"type": "score", "instructions": "How?", "criteria": ["low"]}}}),
        ),
        (
            "question not an object",
            json!({"state": "s", "questions": {"q": "Is it?"}}),
        ),
    ];
    for (label, body) in &edits {
        invalid.push((label, "application/json", serde_json::to_vec(body)?));
    }
    for (label, content_type, body) in invalid {
        let headers = [("content-type", content_type)];
        let reply = send(&app, "POST", "/v1/systemone", &headers, body).await?;
        println!("{label}: {} {}", reply.status, reply.body);
        validation_shape(&reply);
    }
    let reply = post(&app, &json!({"state": "s", "questions": {"q": "Is it?"}})).await?;
    assert_eq!(
        reply.body,
        json!({"detail": [{"loc": ["body", "questions", "q"], "msg": "question must be an object, got 'Is it?'", "type": "value_error"}]})
    );

    // 413: the context window and decider's limits, all before anything loads.
    let too_large: [(&str, Value, &str); 4] = [
        (
            "state over the context window",
            json!({"state": words(41_000), "questions": {"q": noul("Is it?")}}),
            "context window",
        ),
        (
            "1,025 rows",
            json!({"state": "s", "questions": (0..1025).map(|i| (format!("q{i}"), noul("Is it?"))).collect::<serde_json::Map<_, _>>()}),
            "too many questions: the request expands to 1025 scoring rows, the limit is 1024",
        ),
        (
            "a row over 36,864 tokens",
            json!({"state": words(38_000), "questions": {"q": noul("Is it?")}}),
            "too many tokens: one row has",
        ),
        (
            "over 1,048,576 request tokens",
            json!({"state": words(35_000), "questions": (0..31).map(|i| (format!("q{i}"), noul("Is it?"))).collect::<serde_json::Map<_, _>>()}),
            "too many tokens: the request has",
        ),
    ];
    for (label, body, expected) in &too_large {
        let reply = post(&app, body).await?;
        assert_eq!(reply.status, 413, "{label}: {reply:?}");
        let message = error_shape(&reply, "request_too_large");
        assert!(message.contains(expected), "{label}: {message}");
    }
    let huge = vec![b' '; ardana_server::BODY_LIMIT + 1];
    let reply = send(&app, "POST", "/v1/systemone", &json_type, huge).await?;
    assert_eq!(reply.status, 413);
    assert!(
        error_shape(&reply, "request_too_large").contains("32 MiB"),
        "{reply:?}"
    );
    assert_eq!(
        (fake.loads(), fake.decoded().len()),
        (0, 0),
        "nothing loaded or decoded"
    );

    // 503: beyond --max-queued-rows while two rows wait on a held decode.
    fake.close();
    let two_rows =
        json!({"state": "s", "questions": {"a": noul("Is it?"), "b": noul("Is it not?")}});
    let first = tokio::spawn({
        let app = app.clone();
        async move { post(&app, &two_rows).await }
    });
    eventually("two rows are queued", || models.queued_rows() == 2).await?;
    let busy = post(&app, &base).await?;
    assert_eq!(busy.status, 503, "{busy:?}");
    assert_eq!(
        busy.headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok()),
        Some("1")
    );
    assert!(
        error_shape(&busy, "overloaded_error").contains("the limit is 2"),
        "{busy:?}"
    );
    fake.open();
    assert_eq!(first.await??.status, 200);
    assert_eq!(models.queued_rows(), 0);
    assert_eq!(post(&app, &base).await?.status, 200);

    // 403 and 401 with a key; /health stays open.
    let keyed = router(models, Some("secret".into()));
    let body = serde_json::to_vec(&base)?;
    for (headers, status) in [
        (vec![], 403),
        (vec![("authorization", "Bearer ")], 403),
        (vec![("authorization", "Bearer wrong")], 401),
        (vec![("authorization", "Basic c2VjcmV0")], 401),
    ] {
        let mut headers = headers;
        headers.push(("content-type", "application/json"));
        let reply = send(&keyed, "POST", "/v1/systemone", &headers, body.clone()).await?;
        assert_eq!(reply.status, status, "{headers:?}: {reply:?}");
        error_shape(&reply, "authentication_error");
    }
    let reply = send(
        &keyed,
        "GET",
        "/v1/models",
        &[("authorization", "Bearer wrong")],
        Vec::new(),
    )
    .await?;
    assert_eq!(reply.status, 401);
    let authorized = [
        ("authorization", "Bearer secret"),
        ("content-type", "application/json"),
    ];
    assert_eq!(
        send(&keyed, "POST", "/v1/systemone", &authorized, body)
            .await?
            .status,
        200
    );
    assert_eq!(get(&keyed, "/health").await?.status, 200);
    Ok(())
}

/// A loopback library (C2) publishing `index` at `/models.json` and each of its models' manifests at
/// `/models/<name>.json`, 404 elsewhere; the paths it answered, in order.
async fn library_server(index: Value) -> Result<(String, Arc<Mutex<Vec<String>>>)> {
    let models: Arc<Vec<Value>> = Arc::new(index["models"].as_array().cloned().unwrap_or_default());
    let paths: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = paths.clone();
    let manifest = move |Path(file): Path<String>| {
        let models = models.clone();
        async move {
            let entry = file
                .strip_suffix(".json")
                .and_then(|name| models.iter().find(|m| m["name"] == name));
            match entry {
                Some(model) => Json(json!({"schema": 1, "model": model})).into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    };
    let app = Router::new()
        .route(
            "/models.json",
            route_get(move || async move { Json(index) }),
        )
        .route("/models/{file}", route_get(manifest))
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            seen.lock()
                .expect("the request log")
                .push(request.uri().path().to_string());
            next.run(request)
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/models.json", listener.local_addr()?);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((url, paths))
}

/// R4.4: under a URL source, a request naming a model the index lacks sends one manifest GET; on a 404 the answer is
/// the unknown-model 404, and the same name (in any spelling) sends no further GET until the next index refresh. A
/// reference is no library name and asks nothing.
#[tokio::test]
async fn unknown_names_ask_the_library_once() -> Result<()> {
    let snapshot = common::library()?;
    let index: Value = serde_json::from_str(&std::fs::read_to_string(
        std::env::var_os("ARDANA_LIBRARY").context("cargo sets ARDANA_LIBRARY")?,
    )?)?;
    let (url, paths) = library_server(index).await?;
    let (_, models) = common::models_from(
        "http-unknown-names",
        &["alpha"],
        ModelOptions::default(),
        LibrarySource::Url(url),
    )?;
    let app = router(models.clone(), None);
    // The first requests wait for the index, which `ardana serve`'s refresh task reads; here the test reads it.
    models.refresh_library().await;
    let seen = || paths.lock().expect("the request log").clone();
    assert_eq!(seen(), ["/models.json"]);
    let listed = get(&app, "/v1/models").await?.body;
    assert_eq!(
        listed["models"].as_array().map(Vec::len),
        Some(1 + snapshot.models.len())
    );

    let ask = |model: &str| json!({"model": model, "state": "s", "questions": {}});
    let reply = post(&app, &ask("gamma")).await?;
    assert_eq!(reply.status, 404);
    assert_eq!(
        error_shape(&reply, "not_found_error"),
        "no model named \"gamma\"; pulled: alpha; the library at https://ardana.ai/models/ is pulled on first use"
    );
    assert_eq!(seen(), ["/models.json", "/models/gamma.json"]);
    for again in ["gamma", "Gamma", "gamma:q8_0"] {
        let reply = post(&app, &ask(again)).await?;
        assert_eq!(reply.status, 404, "{again}");
        assert_eq!(seen().len(), 2, "{again} asked again");
    }
    let reply = post(&app, &ask("hf.co/test/gamma-GGUF")).await?;
    assert_eq!(reply.status, 404);
    assert_eq!(seen().len(), 2, "a reference is asked of no library");

    models.refresh_library().await;
    assert_eq!(seen().len(), 3);
    assert_eq!(seen()[2], "/models.json");
    let reply = post(&app, &ask("gamma")).await?;
    assert_eq!(reply.status, 404);
    assert_eq!(seen()[3], "/models/gamma.json");
    assert_eq!(seen().len(), 4);
    Ok(())
}

/// The binary's `serve` flags, parsed as `ardana serve` parses them.
#[derive(Debug, Parser)]
struct Serve {
    #[command(flatten)]
    args: ServeArgs,
}

#[test]
fn defaults() -> Result<()> {
    let args = Serve::try_parse_from(["serve"])?.args;
    assert_eq!(args.addr(), "127.0.0.1:8000");
    assert_eq!(args.keep_alive, Duration::from_secs(300));
    assert_eq!(args.library_refresh, Duration::from_secs(3600));
    let opts = args.model_options();
    assert_eq!(
        (
            opts.max_loaded_models,
            opts.max_queued_rows,
            opts.default_model
        ),
        (1, 4096, None)
    );

    let args = Serve::try_parse_from([
        "serve",
        "--host",
        "0.0.0.0",
        "--port",
        "9000",
        "--api-key",
        "k",
        "--default-model",
        "m",
        "--keep-alive",
        "30s",
        "--library-refresh",
        "10m",
        "--max-loaded-models",
        "2",
        "--max-queued-rows",
        "8",
    ])?
    .args;
    assert_eq!(args.addr(), "0.0.0.0:9000");
    assert_eq!(args.api_key.as_deref(), Some("k"));
    assert_eq!(args.library_refresh, Duration::from_secs(600));
    let opts = args.model_options();
    assert_eq!(opts.default_model.as_deref(), Some("m"));
    assert_eq!(
        (
            opts.keep_alive,
            opts.max_loaded_models,
            opts.max_queued_rows
        ),
        (Duration::from_secs(30), 2, 8)
    );
    assert_eq!(
        Serve::try_parse_from(["serve", "--host", "::1"])?
            .args
            .addr(),
        "[::1]:8000"
    );
    assert!(Serve::try_parse_from(["serve", "--max-loaded-models", "0"]).is_err());

    let command = Serve::command();
    let api_key = command
        .get_arguments()
        .find(|arg| arg.get_id() == "api_key")
        .expect("an --api-key argument");
    assert_eq!(
        api_key.get_env().and_then(|e| e.to_str()),
        Some("ARDANA_API_KEY")
    );
    let refresh = command
        .get_arguments()
        .find(|arg| arg.get_id() == "library_refresh")
        .expect("a --library-refresh argument");
    assert_eq!(
        refresh.get_env().and_then(|e| e.to_str()),
        Some(ardana_server::LIBRARY_REFRESH_VAR)
    );

    for (text, duration) in [
        ("250ms", 250),
        ("45", 45_000),
        ("45s", 45_000),
        ("5m", 300_000),
        ("1h", 3_600_000),
    ] {
        assert_eq!(
            ardana_server::parse_duration(text),
            Ok(Duration::from_millis(duration)),
            "{text}"
        );
    }
    assert!(ardana_server::parse_duration("5 days").is_err());
    Ok(())
}
