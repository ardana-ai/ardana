//! R5.7 with a fake runtime: a model loads on its first request, loading past `max_loaded_models` evicts the least
//! recently used, `keep_alive` unloads idle models, and one model's requests decode in arrival order.

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
    let registry = common::registry("lifecycle-fifo-plan", &["alpha"])?;
    let model = registry.resolve("alpha")?;
    let decider = ardana_core::Decider::new(
        ardana_registry::load_tokenizer(&model.tokenizer)?,
        model.profile,
    )?;
    let expected: Vec<Vec<u32>> = states
        .iter()
        .map(|state| {
            let request = serde_json::from_value(ask("alpha", state))?;
            let plan = decider.plan(&request)?;
            Ok(plan.row_ids().next().expect("one row").to_vec())
        })
        .collect::<Result<_>>()?;
    assert_eq!(fake.decoded()[1..], expected[..]);
    Ok(())
}
