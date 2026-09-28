//! R5.7 with a fake runtime: a model loads on its first request, loading past `max_loaded_models` evicts the least
//! recently used (never holding more models than the limit), `keep_alive` unloads idle models, one model's requests
//! decode in arrival order, a load holds up only its own model, and a client that goes away keeps its queued rows
//! counted (R5.6's 503) until its job is decoded.

mod common;

use std::time::Duration;

use anyhow::Result;
use ardana_server::{ModelOptions, router};
use common::{eventually, get, models, post};
use serde_json::{Value, json};

fn ask(model: &str, state: &str) -> Value {
    json!({"model": model, "state": state, "questions": {"q": {"type": "noul", "instructions": "Is it?"}}})
}

#[tokio::test]
async fn fake_runtime_loads_on_first_request() -> Result<()> {
    let (fake, models) = models(
        "lifecycle-first-request",
        &["alpha"],
        ModelOptions::default(),
    )?;
    let app = router(models.clone(), None);
    get(&app, "/health").await?;
    get(&app, "/v1/models").await?;
    assert_eq!(
        (fake.loads(), models.loaded().await),
        (0, Vec::<String>::new())
    );
    assert_eq!(post(&app, &ask("alpha", "one")).await?.status, 200);
    assert_eq!(
        (fake.loads(), models.loaded().await),
        (1, vec!["alpha".to_string()])
    );
    assert_eq!(post(&app, &ask("jev-latest", "two")).await?.status, 200);
    assert_eq!(
        (fake.loads(), fake.decoded().len()),
        (1, 2),
        "the loaded model answers again"
    );
    Ok(())
}

#[tokio::test]
async fn fake_runtime_evicts_least_recently_used() -> Result<()> {
    let (fake, models) = models(
        "lifecycle-lru-one",
        &["alpha", "beta"],
        ModelOptions::default(),
    )?;
    let app = router(models.clone(), None);
    post(&app, &ask("alpha", "s")).await?;
    post(&app, &ask("beta", "s")).await?;
    assert_eq!(models.loaded().await, ["beta"]);
    eventually("alpha is dropped", || fake.drops() == 1).await?;
    assert!(fake.dropped.lock().unwrap()[0].ends_with("alpha.fake"));

    let opts = ModelOptions {
        max_loaded_models: 2,
        ..ModelOptions::default()
    };
    let (fake, models) = common::models("lifecycle-lru-two", &["alpha", "beta", "gamma"], opts)?;
    let app = router(models.clone(), None);
    for model in ["alpha", "beta", "alpha", "gamma"] {
        assert_eq!(post(&app, &ask(model, "s")).await?.status, 200);
    }
    assert_eq!(models.loaded().await, ["gamma", "alpha"]);
    eventually("beta is dropped", || fake.drops() == 1).await?;
    assert!(fake.dropped.lock().unwrap()[0].ends_with("beta.fake"));
    assert_eq!(fake.loads(), 3);
    Ok(())
}

#[tokio::test]
async fn fake_runtime_keep_alive_unloads_idle_models() -> Result<()> {
    let opts = ModelOptions {
        keep_alive: Duration::from_millis(300),
        ..ModelOptions::default()
    };
    let (fake, models) = models("lifecycle-keep-alive", &["alpha"], opts)?;
    let app = router(models.clone(), None);
    post(&app, &ask("alpha", "s")).await?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    post(&app, &ask("alpha", "s")).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        models.loaded().await,
        ["alpha"],
        "a request restarts the idle time"
    );
    let loaded = models.clone();
    for _ in 0..200 {
        if loaded.loaded().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        models.loaded().await.is_empty(),
        "alpha is unloaded after 300 ms idle"
    );
    eventually("alpha is dropped", || fake.drops() == 1).await?;

    post(&app, &ask("alpha", "s")).await?;
    assert_eq!(
        (fake.loads(), models.loaded().await),
        (2, vec!["alpha".to_string()]),
        "it loads again"
    );
    Ok(())
}

#[tokio::test]
async fn fake_runtime_requests_run_fifo() -> Result<()> {
    let (fake, models) = models("lifecycle-fifo", &["alpha"], ModelOptions::default())?;
    let app = router(models.clone(), None);
    post(&app, &ask("alpha", "warm up")).await?;
    fake.close();
    let states: Vec<String> = (1..=6).map(|i| format!("request {i}")).collect();
    let mut pending = Vec::new();
    for (i, state) in states.iter().enumerate() {
        let (app, body) = (app.clone(), ask("alpha", state));
        pending.push(tokio::spawn(async move { post(&app, &body).await }));
        eventually("the request is queued", || models.queued_rows() == i + 1).await?;
    }
    fake.open();
    for reply in pending {
        assert_eq!(reply.await??.status, 200);
    }

    // The order the fake decoded in, against each request's own prompt row.
    assert_eq!(
        fake.decoded()[1..],
        rows("lifecycle-fifo-plan", &states)?[..]
    );
    Ok(())
}

/// The prompt row of `ask("alpha", state)` for each state.
fn rows(test: &str, states: &[String]) -> Result<Vec<Vec<u32>>> {
    let registry = common::registry(test, &["alpha"])?;
    let model = registry.resolve("alpha")?;
    let decider = ardana_core::Decider::new(
        ardana_registry::load_tokenizer(&model.tokenizer)?,
        model.profile,
    )?;
    states
        .iter()
        .map(|state| {
            let request = serde_json::from_value(ask("alpha", state))?;
            let plan = decider.plan(&request)?;
            Ok(plan.row_ids().next().expect("one row").to_vec())
        })
        .collect()
}

/// Fails instead of hanging when `future` waits on something it must not.
async fn soon<T>(what: &str, future: impl Future<Output = T>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .map_err(|_| anyhow::anyhow!("{what} waited over 5 s"))
}

#[tokio::test]
async fn fake_runtime_load_blocks_only_its_model() -> Result<()> {
    let opts = ModelOptions {
        max_loaded_models: 2,
        ..ModelOptions::default()
    };
    let (fake, models) = models("lifecycle-load-blocks", &["alpha", "beta"], opts)?;
    let _open = fake.open_on_drop();
    let app = router(models.clone(), None);
    post(&app, &ask("alpha", "warm up")).await?;

    // beta's load is held; its later requests queue behind it in arrival order.
    fake.close_loads();
    let states: Vec<String> = (1..=3).map(|i| format!("request {i}")).collect();
    let mut pending = Vec::new();
    for (i, state) in states.iter().enumerate() {
        let (app, body) = (app.clone(), ask("beta", state));
        pending.push(tokio::spawn(async move { post(&app, &body).await }));
        eventually("beta is loading", || fake.loads() == 2).await?;
        eventually("the request waits", || models.queued_rows() == i + 1).await?;
    }

    let health = soon("/health during a load", get(&app, "/health")).await??;
    assert_eq!(health.body["x_loaded"], json!(["alpha"]));
    let loaded = soon(
        "a loaded model during a load",
        post(&app, &ask("alpha", "s")),
    )
    .await??;
    assert_eq!(loaded.status, 200);

    fake.open_loads();
    for reply in pending {
        assert_eq!(reply.await??.status, 200);
    }
    assert_eq!(fake.loads(), 2, "beta loads once");
    let decoded = fake.decoded();
    assert_eq!(
        decoded[decoded.len() - 3..],
        rows("lifecycle-load-blocks-plan", &states)?[..],
        "beta's requests decode in arrival order"
    );
    Ok(())
}

#[tokio::test]
async fn fake_runtime_eviction_waits_for_busy_models() -> Result<()> {
    let (fake, models) = models(
        "lifecycle-evict-busy",
        &["alpha", "beta"],
        ModelOptions::default(),
    )?;
    let _open = fake.open_on_drop();
    let app = router(models.clone(), None);
    post(&app, &ask("alpha", "warm up")).await?;

    // alpha is decoding when beta, then alpha again, are asked for.
    fake.close();
    let mut pending = Vec::new();
    for (i, model) in ["alpha", "beta", "alpha"].into_iter().enumerate() {
        let (app, body) = (app.clone(), ask(model, "s"));
        pending.push(tokio::spawn(async move { post(&app, &body).await }));
        eventually("the request is admitted", || models.queued_rows() == i + 1).await?;
    }
    fake.open();
    for reply in pending {
        assert_eq!(reply.await??.status, 200);
    }
    assert_eq!(fake.peak_resident(), 1, "one model is resident at a time");
    Ok(())
}

#[tokio::test]
async fn fake_runtime_disconnected_requests_keep_their_rows() -> Result<()> {
    let opts = ModelOptions {
        max_queued_rows: 2,
        ..ModelOptions::default()
    };
    let (fake, models) = models("lifecycle-disconnect", &["alpha"], opts)?;
    let _open = fake.open_on_drop();
    let app = router(models.clone(), None);
    post(&app, &ask("alpha", "warm up")).await?;

    // One request decodes (held), one waits in the queue; the waiting client goes away.
    fake.close();
    let decoding = tokio::spawn({
        let (app, body) = (app.clone(), ask("alpha", "decoding"));
        async move { post(&app, &body).await }
    });
    eventually("one row is queued", || models.queued_rows() == 1).await?;
    let queued = tokio::spawn({
        let (app, body) = (app.clone(), ask("alpha", "queued"));
        async move { post(&app, &body).await }
    });
    eventually("two rows are queued", || models.queued_rows() == 2).await?;
    queued.abort();
    assert!(queued.await.is_err_and(|err| err.is_cancelled()));

    assert_eq!(models.queued_rows(), 2, "the abandoned job is still queued");
    let busy = soon(
        "a request over the row limit",
        post(&app, &ask("alpha", "s")),
    )
    .await??;
    assert_eq!(busy.status, 503, "{busy:?}");

    fake.open();
    assert_eq!(decoding.await??.status, 200);
    eventually("every row is released", || models.queued_rows() == 0).await?;
    assert_eq!(fake.decoded().len(), 3, "the abandoned job still decoded");
    Ok(())
}
