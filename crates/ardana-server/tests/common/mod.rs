//! Shared helpers of the server tests: a fake runtime that counts loads and drops and can hold a load or a decode, registries
//! of decider:2b's real tokenizer and profile (from `tmp/hf`) over fake weights, a decision-model entry (Gemma 4's
//! real tokenizer, a profile read as THX-01's is) over fake weights of the other kind, and requests through the router.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use ardana_core::{
    DecisionModel, DecisionSequence, LoadOptions, Loaded, LoadedModel, Runtime, Runtimes,
};
use ardana_registry::library::{Library, LibraryClient, LibrarySource};
use ardana_registry::{LayoutKind, Registry, ResolvedModel};
use ardana_server::{ModelOptions, Models};
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

/// What the fake runtime saw: loads, drops, and every decoded prompt (or decision sequence) in decode order.
#[derive(Debug, Default)]
pub struct Fake {
    pub loads: AtomicUsize,
    pub drops: AtomicUsize,
    /// The most models loaded and not yet dropped at once.
    pub peak_resident: AtomicUsize,
    /// The weights file of each load and each drop, in order.
    pub loaded: Mutex<Vec<PathBuf>>,
    pub dropped: Mutex<Vec<PathBuf>>,
    /// The token ids of every decoded prompt row, and of every sequence a decision model was asked.
    pub decoded: Mutex<Vec<Vec<u32>>>,
    /// While closed, every decode waits.
    decode_gate: Gate,
    /// While closed, every load waits (after it is counted).
    load_gate: Gate,
}

/// A door threads wait at while it is closed; it starts open.
#[derive(Debug, Default)]
struct Gate {
    closed: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn set(&self, closed: bool) {
        *self.closed.lock().unwrap() = closed;
        self.opened.notify_all();
    }

    fn pass(&self) {
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.opened.wait(closed).unwrap();
        }
    }
}

impl Fake {
    pub fn new() -> Arc<Fake> {
        Arc::new(Fake::default())
    }

    pub fn runtimes(self: &Arc<Fake>) -> Runtimes {
        Runtimes(vec![Box::new(FakeRuntime(self.clone()))])
    }

    /// Holds every decode until [`Fake::open`].
    pub fn close(&self) {
        self.decode_gate.set(true);
    }

    pub fn open(&self) {
        self.decode_gate.set(false);
    }

    /// Holds every load until [`Fake::open_loads`].
    pub fn close_loads(&self) {
        self.load_gate.set(true);
    }

    pub fn open_loads(&self) {
        self.load_gate.set(false);
    }

    /// Opens both gates when the guard drops, so a failing test leaves no load waiting (the runtime would wait for
    /// it) and no worker thread held.
    pub fn open_on_drop(self: &Arc<Fake>) -> OpenOnDrop {
        OpenOnDrop(self.clone())
    }

    pub fn loads(&self) -> usize {
        self.loads.load(Ordering::SeqCst)
    }

    pub fn drops(&self) -> usize {
        self.drops.load(Ordering::SeqCst)
    }

    pub fn peak_resident(&self) -> usize {
        self.peak_resident.load(Ordering::SeqCst)
    }

    pub fn decoded(&self) -> Vec<Vec<u32>> {
        self.decoded.lock().unwrap().clone()
    }
}

/// See [`Fake::open_on_drop`].
pub struct OpenOnDrop(Arc<Fake>);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
        self.0.open_loads();
    }
}

struct FakeRuntime(Arc<Fake>);

impl Runtime for FakeRuntime {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn supports(&self, _weights: &Path) -> bool {
        true
    }

    fn load(&self, weights: &Path, opts: &LoadOptions) -> anyhow::Result<Loaded> {
        let loads = self.0.loads.fetch_add(1, Ordering::SeqCst) + 1;
        let resident = loads - self.0.drops.load(Ordering::SeqCst);
        self.0.peak_resident.fetch_max(resident, Ordering::SeqCst);
        self.0.loaded.lock().unwrap().push(weights.to_path_buf());
        self.0.load_gate.pass();
        let model = Box::new(FakeModel {
            fake: self.0.clone(),
            weights: weights.to_path_buf(),
            n_ctx: opts.n_ctx as usize,
        });
        Ok(match weights.extension() {
            Some(kind) if kind == DECISION_WEIGHTS => Loaded::Decision(model),
            _ => Loaded::Causal(model),
        })
    }
}

/// The extension of the weights file the fake runtime loads as a decision model; any other file is a causal one.
const DECISION_WEIGHTS: &str = "decision";

/// Answers every slot with equal logits, and every sequence with equal probabilities, after the gate opens.
struct FakeModel {
    fake: Arc<Fake>,
    weights: PathBuf,
    n_ctx: usize,
}

impl LoadedModel for FakeModel {
    fn n_ctx(&self) -> usize {
        self.n_ctx
    }

    fn slot_logits(
        &mut self,
        ids: &[u32],
        slots: &[usize],
        label_ids: &[u32],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        self.fake.decode_gate.pass();
        self.fake.decoded.lock().unwrap().push(ids.to_vec());
        Ok(slots.iter().map(|_| vec![0.0; label_ids.len()]).collect())
    }
}

impl DecisionModel for FakeModel {
    fn decide(&mut self, sequences: &[DecisionSequence]) -> anyhow::Result<Vec<Vec<f64>>> {
        self.fake.decode_gate.pass();
        let mut decoded = self.fake.decoded.lock().unwrap();
        Ok(sequences
            .iter()
            .map(|sequence| {
                decoded.push(sequence.ids.clone());
                let options = sequence.markers.len();
                vec![1.0 / options as f64; options]
            })
            .collect())
    }
}

impl Drop for FakeModel {
    fn drop(&mut self) {
        self.fake.dropped.lock().unwrap().push(self.weights.clone());
        self.fake.drops.fetch_add(1, Ordering::SeqCst);
    }
}

/// A file of the pinned snapshot of `repo` in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
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

/// The `/v1/models` rows of every library size whose canonical name is not in `pulled`, in document order (R3.1): the
/// default quant's reference and bytes, a browser variant's bytes, the document's browser default, and `x_default` on
/// `default` when it names one of them.
pub fn library_rows(pulled: &[&str], default: Option<&str>) -> Result<Vec<Value>> {
    let library = library()?;
    Ok(library
        .sizes()
        .iter()
        .filter(|pick| !pulled.contains(&pick.name().as_str()))
        .map(|pick| {
            let name = pick.name();
            let mut row = serde_json::json!({
                "name": name, "description": pick.reference(), "release_date": pick.size.release_date,
                "x_pulled": false, "x_size": pick.quant.bytes,
            });
            if default == Some(name.as_str()) {
                row["x_default"] = Value::Bool(true);
            }
            if let Some(browser) = &pick.size.browser {
                row["x_browser"] = Value::from(browser.bytes);
            }
            if library.browser_default.as_deref() == Some(name.as_str()) {
                row["x_browser_default"] = Value::Bool(true);
            }
            row
        })
        .collect())
}

/// `listed`, a `/v1/models` body as a test expects it, with `x_browser_pulled` on every model whose browser variant the
/// hub cache (`tmp/hf`) holds whole: all three once `cargo xtask onnx convert` has built them, none where `cargo xtask
/// fetch --tests` fetched no ONNX file (CI).
pub async fn with_held_variants(mut listed: Value) -> Result<Value> {
    let cache = ardana_registry::BrowserCache::from_env()?;
    let library = library()?;
    for model in listed["models"].as_array_mut().context("models")? {
        let name = model["name"].as_str().unwrap_or_default().to_string();
        if let Some(pick) = library.browser(&name)
            && model.get("x_browser").is_some()
            && cache.holds(&pick).await
        {
            model["x_browser_pulled"] = Value::Bool(true);
        }
    }
    Ok(listed)
}

/// `$ARDANA_TMP/<name>`, emptied.
pub fn scratch(name: &str) -> Result<PathBuf> {
    let tmp = PathBuf::from(std::env::var_os("ARDANA_TMP").context("ARDANA_TMP is not set")?);
    let dir = tmp.join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A registry in `$ARDANA_TMP/<test>/home` with one entry per name, saved to its `models.toml`: decider:2b's
/// tokenizer and `decider_config.json` profile (renamed after the entry, so responses tell the models apart) over an
/// empty fake weights file `<name>.fake`. The first entry keeps decider:2b's release date; the others have none.
pub fn registry(test: &str, names: &[&str]) -> Result<Registry> {
    let dir = scratch(test)?;
    let mut registry = Registry::open(&dir.join("home"))?;
    for name in names {
        registry.insert(fake_entry(&dir, name)?);
    }
    if let Some(first) = names.first() {
        let mut model = registry.entry(first)?.clone();
        model.profile.release_date = Some("2026-09-24".into());
        registry.insert(model);
    }
    registry.save()?;
    Ok(registry)
}

/// A registry entry named `name` over the fake weights `<dir>/<name>.fake` (created), with decider:2b's tokenizer and
/// `decider_config.json` profile renamed after the entry and no release date.
pub fn fake_entry(dir: &Path, name: &str) -> Result<ResolvedModel> {
    let repo = "Mapika/decider-2b-GGUF";
    let tokenizer_path = hf_file(repo, "tokenizer.json")?;
    let config = hf_file(repo, "decider_config.json")?;
    // Loading the 20 MB tokenizer takes about a second in a debug build: the profile is read once per test binary.
    static PROFILE: std::sync::OnceLock<ardana_core::ModelProfile> = std::sync::OnceLock::new();
    let mut profile = match PROFILE.get() {
        Some(profile) => profile.clone(),
        None => {
            let tokenizer = ardana_registry::load_tokenizer(&tokenizer_path)?;
            let profile = ardana_registry::read_profile(
                &tokenizer_path,
                &tokenizer,
                Some(&config),
                LayoutKind::Plain,
                name,
            )?;
            PROFILE.get_or_init(|| profile).clone()
        }
    };
    let weights = dir.join(format!("{name}.fake"));
    std::fs::write(&weights, b"")?;
    profile.name = name.to_string();
    profile.release_date = None;
    Ok(ResolvedModel {
        name: name.to_string(),
        source: format!("hf.co/test/{name}-GGUF"),
        weights,
        tokenizer: tokenizer_path,
        runtime: "fake".into(),
        profile,
        pulled_at: "2026-09-28".into(),
    })
}

/// THX-01's `rl_agent_config.json` limits (a 1,024-token window, 24 options, a 256-token head) with a base
/// temperature triple (choice, score, noul) whose last value the profile clamps to 0.5.
pub fn decision_config() -> Value {
    serde_json::json!({"max_len": 1024, "head_max_len": 256, "tournament_above": 24, "auto_head_budget": true,
                       "temperature": [2.55, 3.05, 0.1]})
}

/// A registry entry named `name` for a decision model over the fake weights `<dir>/<name>.decision` (created), which
/// the fake runtime loads as one: the profile is [`decision_config`] read as a pull reads THX-01's, renamed after the
/// entry, and the tokenizer is Gemma 4's, which names `<bos>`, `<eos>` and `<mask>` as THX-01's Gemma tokenizer does.
pub fn decision_entry(dir: &Path, name: &str) -> Result<ResolvedModel> {
    let weights = dir.join(format!("{name}.{DECISION_WEIGHTS}"));
    std::fs::write(&weights, b"")?;
    let mut profile = ardana_core::from_rl_agent_config(&decision_config())?;
    profile.name = name.to_string();
    Ok(ResolvedModel {
        name: name.to_string(),
        source: format!("hf.co/test/{name}"),
        weights,
        tokenizer: hf_file("google/gemma-4-E2B-it", "tokenizer.json")?,
        runtime: "fake".into(),
        profile,
        pulled_at: "2026-10-12".into(),
    })
}

/// [`Models`] over [`registry`] with the fake runtime, reading the library snapshot (a file source).
pub fn models(test: &str, names: &[&str], opts: ModelOptions) -> Result<(Arc<Fake>, Arc<Models>)> {
    models_from(test, names, opts, LibrarySource::from_env())
}

/// [`models`] over a registry that holds the decision model `decision` ([`decision_entry`]) after `names`.
pub fn models_with_decision(
    test: &str,
    names: &[&str],
    decision: &str,
    opts: ModelOptions,
) -> Result<(Arc<Fake>, Arc<Models>)> {
    let mut registry = registry(test, names)?;
    let dir = registry
        .path()
        .parent()
        .and_then(Path::parent)
        .context("the test's scratch directory")?
        .to_path_buf();
    registry.insert(decision_entry(&dir, decision)?);
    registry.save()?;
    models_over(registry, opts, LibrarySource::from_env())
}

/// [`Models`] over [`registry`] with the fake runtime, reading the library from `source`, cached in the registry's
/// home.
pub fn models_from(
    test: &str,
    names: &[&str],
    opts: ModelOptions,
    source: LibrarySource,
) -> Result<(Arc<Fake>, Arc<Models>)> {
    models_over(registry(test, names)?, opts, source)
}

/// [`Models`] over `registry` with the fake runtime, reading the library from `source`, cached in the registry's home.
fn models_over(
    registry: Registry,
    opts: ModelOptions,
    source: LibrarySource,
) -> Result<(Arc<Fake>, Arc<Models>)> {
    let fake = Fake::new();
    let home = registry
        .path()
        .parent()
        .context("the registry's home")?
        .to_path_buf();
    let client = LibraryClient::new(source, &home);
    let models = Models::new(registry, client, fake.runtimes(), opts)?;
    Ok((fake, Arc::new(models)))
}

/// The library snapshot (C5), which cargo's `[env]` names as `ARDANA_LIBRARY` for every test: what the server reads
/// under a file source.
pub fn library() -> Result<Library> {
    let path = std::env::var_os(ardana_registry::library::LIBRARY_VAR)
        .context("cargo sets ARDANA_LIBRARY")?;
    Ok(Library::from_file(Path::new(&path))?)
}

/// A response: status, headers and the JSON body.
#[derive(Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

/// Sends `method uri` with `headers` and `body` through `app`; the body must be JSON.
pub async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<Reply> {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app.clone().oneshot(request.body(Body::from(body))?).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
    let body = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "{method} {uri} ({status}): {}",
            String::from_utf8_lossy(&bytes)
        )
    })?;
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    ensure!(
        content_type == "application/json",
        "{method} {uri}: content-type {content_type:?}"
    );
    Ok(Reply {
        status,
        headers,
        body,
    })
}

/// `POST /v1/systemone` with a JSON body.
pub async fn post(app: &Router, body: &Value) -> Result<Reply> {
    send(
        app,
        "POST",
        "/v1/systemone",
        &[("content-type", "application/json")],
        serde_json::to_vec(body)?,
    )
    .await
}

/// `GET uri`.
pub async fn get(app: &Router, uri: &str) -> Result<Reply> {
    send(app, "GET", uri, &[], Vec::new()).await
}

/// Polls `check` every 10 ms for up to 10 s.
pub async fn eventually(what: &str, mut check: impl FnMut() -> bool) -> Result<()> {
    for _ in 0..1000 {
        if check() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("timed out waiting until {what}")
}
