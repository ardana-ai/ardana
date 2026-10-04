//! [`Models`]: registry models behind the API, pulled and loaded on their first request.
//!
//! The registry is `models.toml` as last read: it is reread whenever the file's modification time changes, so a
//! model `ardana pull` adds while the server runs is servable at once. A request naming a library model that is not
//! pulled yet (the library default, for `jev-*` and no model, while the registry is empty) pulls it first; requests
//! for the same model share one pull, which finishes even when its requester goes away.
//!
//! A library model's browser variant is pulled the same way, into the hub cache only, by the first request for its
//! files ([`Models::browser`]).
//!
//! Every model reads its requests through a [`Decider`] (tokenizer and profile, built on the first request and kept
//! while its registry entry is unchanged), so a request is validated and sized before any weights load. Loaded weights live on one worker thread per model
//! that decodes one request at a time from a FIFO channel. At most `max_loaded_models` hold weights at once, counting
//! models still loading and unloaded ones still answering their queue; loading one more evicts the least recently
//! used and waits until its weights are dropped, and a model idle for `keep_alive` is unloaded. Unloading closes the
//! worker's channel, so the worker finishes the requests it already has before it drops the model. A load holds only
//! its own model's turn, so other models and `/health` answer meanwhile. Admission counts the scoring rows of queued
//! and decoding requests over all models, until the worker is done with them, and refuses a request that would exceed
//! `max_queued_rows`.

use std::collections::HashMap;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime};

use ardana_api::{ModelInfo, ModelsResponse, SystemOneRequest, SystemOneResponse};
use ardana_core::{DecideError, Decider, Limits, LoadOptions, LoadedModel, Plan, Runtimes};
use ardana_registry::library::library;
use ardana_registry::{
    BrowserCache, BrowserModel, Named, PullOptions, Registry, RegistryError, ResolvedModel,
};
use serde_json::{Map, Value, json};
use tokio::sync::{Notify, mpsc, oneshot, watch};

use crate::browser;
use crate::error::ApiError;

/// Every `jev-*` model name, `jev-latest` included, means the default model (Q7): the aliases TypeSafe SDKs and Jev
/// clients send.
const JEV_PREFIX: &str = "jev-";

/// How [`Models`] loads and schedules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOptions {
    /// The model `jev-*` names and requests without a model resolve to, a registry entry or a library model; `None`
    /// is the first registry entry, else the library default.
    pub default_model: Option<String>,
    pub load: LoadOptions,
    /// How long a model stays loaded after its last request.
    pub keep_alive: Duration,
    /// Models loaded at once, at least 1.
    pub max_loaded_models: usize,
    /// Scoring rows admitted and not yet answered, over all models, at least 1.
    pub max_queued_rows: usize,
}

impl Default for ModelOptions {
    /// Ollama's 5-minute keep-alive, one loaded model and decider's 4,096 queued rows.
    fn default() -> Self {
        ModelOptions {
            default_model: None,
            load: LoadOptions::default(),
            keep_alive: Duration::from_secs(300),
            max_loaded_models: 1,
            max_queued_rows: 4096,
        }
    }
}

/// Why [`Models::new`] refuses its options.
#[derive(Debug, thiserror::Error)]
pub enum ModelsError {
    #[error("--default-model: {0}")]
    DefaultModel(RegistryError),
    #[error("{0} must be at least 1")]
    Zero(&'static str),
}

pub struct Models {
    catalog: Arc<Mutex<Catalog>>,
    runtimes: Arc<Runtimes>,
    opts: ModelOptions,
    /// `--default-model` as the registry names it (`decider-2b` for `decider-2b:Q4_K_M`).
    default: Option<String>,
    /// Per model, the reader built from its registry entry.
    deciders: Mutex<HashMap<String, (ResolvedModel, Arc<Decider>)>>,
    /// Library models being pulled into the registry, which holds them once pulled.
    pulls: Pulls<()>,
    /// The browser variants pulled, or being pulled, for the playground's in-tab engine.
    browsers: Pulls<Arc<BrowserModel>>,
    /// The hub cache, asked which browser variants it holds whole (`x_browser_pulled`); none where the Hugging Face
    /// client cannot be built, as a pull could not run either.
    browser_cache: Option<BrowserCache>,
    /// Per model, the turn a request holds from looking up its worker (loading it if needed) to queueing on it, so
    /// one model's requests queue in arrival order and it loads once.
    turns: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Loaded models taking requests; never locked across a load.
    loaded: Arc<tokio::sync::Mutex<Vec<Arc<Worker>>>>,
    residents: Arc<Residents>,
    queued_rows: Arc<AtomicUsize>,
}

impl std::fmt::Debug for Models {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Models")
            .field("names", &lock(&self.catalog).registry.names())
            .field("runtimes", &self.runtimes)
            .field("opts", &self.opts)
            .finish()
    }
}

impl Models {
    pub fn new(
        registry: Registry,
        runtimes: Runtimes,
        opts: ModelOptions,
    ) -> Result<Models, ModelsError> {
        if opts.max_loaded_models == 0 {
            return Err(ModelsError::Zero("the loaded-model limit"));
        }
        if opts.max_queued_rows == 0 {
            return Err(ModelsError::Zero("the queued-row limit"));
        }
        let default = match &opts.default_model {
            Some(name) => Some(
                match registry.named(name).map_err(ModelsError::DefaultModel)? {
                    Named::Pulled(model) => model.name.clone(),
                    Named::Library(pick) => pick.name(),
                },
            ),
            None => None,
        };
        let residents = Arc::new(Residents {
            count: AtomicUsize::new(0),
            limit: opts.max_loaded_models,
            changed: Notify::new(),
        });
        Ok(Models {
            catalog: Arc::new(Mutex::new(Catalog {
                modified: registry.modified(),
                registry: Arc::new(registry),
            })),
            runtimes: Arc::new(runtimes),
            opts,
            default,
            deciders: Mutex::new(HashMap::new()),
            pulls: Pulls::forgotten(),
            browsers: Pulls::kept(),
            browser_cache: BrowserCache::from_env().ok(),
            turns: Mutex::new(HashMap::new()),
            loaded: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            residents,
            queued_rows: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// The registry as `models.toml` holds it now: reread when the file's modification time changed since the last
    /// read.
    pub fn registry(&self) -> Result<Arc<Registry>, ApiError> {
        lock(&self.catalog)
            .current()
            .map_err(|err| ApiError::Internal(err.to_string()))
    }

    /// The model `jev-*` names and requests without a model mean: `--default-model`, else the first registry entry,
    /// else the library default.
    fn default_name(&self, registry: &Registry) -> String {
        self.default
            .clone()
            .or_else(|| registry.names().into_iter().next())
            .unwrap_or_else(|| library().default.clone())
    }

    /// The registry entry a request's `model` means, pulling a library model that is not pulled yet: `jev-*` and no
    /// model are the default model (Q7); a name that is neither pulled nor in the library is a 404 listing both.
    pub async fn resolve(&self, requested: Option<&str>) -> Result<ResolvedModel, ApiError> {
        let registry = self.registry()?;
        let name = match requested {
            Some(name) if !name.starts_with(JEV_PREFIX) => name.to_string(),
            _ => self.default_name(&registry),
        };
        let name = match registry.named(&name) {
            Ok(Named::Pulled(model)) => model.name.clone(),
            Ok(Named::Library(pick)) => {
                let name = pick.name();
                self.pull(&name).await?;
                name
            }
            Err(err) => return Err(ApiError::UnknownModel(err.to_string())),
        };
        self.registry()?
            .resolve(&name)
            .map_err(|err| ApiError::Internal(err.to_string()))
    }

    /// Pulls the library model `name` into the registry, or waits for the pull already under way.
    async fn pull(&self, name: &str) -> Result<(), ApiError> {
        let home = lock(&self.catalog).home();
        let runtimes = self.runtimes.clone();
        let pulling = name.to_string();
        self.pulls
            .get(name, &format!("model {name}"), || async move {
                let outcome = pull_into(&pulling, &home, runtimes).await;
                if let Err(err) = &outcome {
                    eprintln!("ardana serve: pulling {pulling} failed: {err}");
                }
                outcome
            })
            .await
    }

    /// The browser variant of the library model `name` (one with a `[model.browser]` table), pulled into the hub
    /// cache by the first request for it; requests for the same model share that pull.
    pub async fn browser(&self, name: &str) -> Result<Arc<BrowserModel>, ApiError> {
        let what = format!("the browser variant of {name}");
        self.browsers
            .get(name, &what, || browser::pull(name.to_string()))
            .await
    }

    /// Forgets the browser variant of `name`, whose files went missing: the next request pulls it again.
    pub(crate) fn forget_browser(&self, name: &str) {
        self.browsers.forget(name);
    }

    /// `GET /v1/models`: every registry entry, described by its source reference and dated by its profile's release
    /// date, else the day it was pulled, then every library model not pulled yet, described by the reference it
    /// pulls, with its download size. `x_pulled` tells them apart and `x_default` marks the default model; `x_browser`
    /// gives the download size of the browser variant of a library model that has one, pulled or not,
    /// `x_browser_pulled` marks a browser variant the hub cache holds whole (a tab's request for its files pulls
    /// nothing), and `x_browser_default` marks the library's browser default.
    pub async fn list(&self) -> Result<ModelsResponse, ApiError> {
        let registry = self.registry()?;
        let default = self.default_name(&registry);
        let pulled = registry.entries().iter().map(|m| ModelInfo {
            name: m.name.clone(),
            description: m.source.clone(),
            release_date: m
                .profile
                .release_date
                .clone()
                .unwrap_or_else(|| m.pulled_at.clone()),
            x_pulled: Some(true),
            x_default: m.name == default,
            x_size: None,
            x_browser: library().browser_size(&m.name),
            x_browser_pulled: false,
            x_browser_default: library().is_browser_default(&m.name),
        });
        let pullable = library()
            .models
            .iter()
            .filter(|m| registry.entry(&m.name).is_err())
            .filter_map(|m| library().find(&m.name))
            .map(|pick| {
                let name = pick.name();
                ModelInfo {
                    description: pick.reference(),
                    release_date: pick.model.release_date.clone(),
                    x_pulled: Some(false),
                    x_default: name == default,
                    x_size: pick.size(),
                    x_browser: library().browser_size(&name),
                    x_browser_pulled: false,
                    x_browser_default: library().is_browser_default(&name),
                    name,
                }
            });
        let mut models: Vec<ModelInfo> = pulled.chain(pullable).collect();
        if let Some(cache) = &self.browser_cache {
            for model in models.iter_mut().filter(|m| m.x_browser.is_some()) {
                model.x_browser_pulled = cache.holds(&model.name).await;
            }
        }
        Ok(ModelsResponse { models })
    }

    /// `GET /health`: `status` plus the loaded models (most recently used first) and every model's temperatures.
    pub async fn health(&self) -> Result<Value, ApiError> {
        let temperatures: Map<String, Value> = self
            .registry()?
            .entries()
            .iter()
            .map(|m| {
                let by_type: Map<String, Value> = m
                    .profile
                    .effective_temperatures()
                    .into_iter()
                    .map(|(t, value)| (t.as_str().to_string(), json!(value)))
                    .collect();
                (m.name.clone(), Value::Object(by_type))
            })
            .collect();
        Ok(json!({
            "status": "ok",
            "x_loaded": self.loaded().await,
            "x_temperature_by_type": temperatures,
        }))
    }

    /// The loaded models, most recently used first.
    pub async fn loaded(&self) -> Vec<String> {
        let mut loaded: Vec<(Instant, String)> = self
            .loaded
            .lock()
            .await
            .iter()
            .map(|w| (w.last_used(), w.model.name.clone()))
            .collect();
        loaded.sort_by_key(|(used, _)| std::cmp::Reverse(*used));
        loaded.into_iter().map(|(_, name)| name).collect()
    }

    /// Scoring rows admitted and not yet decoded or discarded.
    pub fn queued_rows(&self) -> usize {
        self.queued_rows.load(Ordering::SeqCst)
    }

    /// `POST /v1/systemone`: resolve the model, plan (validation and the 413 limits, before anything loads), admit
    /// the rows, load the model if needed and queue the request on its worker. The job holds the admitted rows, so
    /// they stay counted until the worker is done with it, even when the client goes away.
    pub async fn decide(&self, req: SystemOneRequest) -> Result<SystemOneResponse, ApiError> {
        let model = self.resolve(req.model.as_deref()).await?;
        let decider = self.decider(&model).await?;
        let planner = decider.clone();
        let plan = tokio::task::spawn_blocking(move || planner.plan(&req))
            .await
            .map_err(|err| ApiError::Internal(format!("planning the request failed: {err}")))??;
        let admission = self.admit(plan.rows())?;
        let (_lease, reply) = self.enqueue(&model, &decider, plan, admission).await?;
        let answer = reply.await.map_err(|_| {
            ApiError::Internal(format!("the worker of model {} stopped", model.name))
        })?;
        Ok(answer?)
    }

    /// The model's reader, built on its first request and again when its registry entry changes.
    async fn decider(&self, model: &ResolvedModel) -> Result<Arc<Decider>, ApiError> {
        if let Some((built_from, decider)) = lock(&self.deciders).get(&model.name)
            && built_from == model
        {
            return Ok(decider.clone());
        }
        let entry = model.clone();
        let model = model.clone();
        let limits = Limits {
            context_window: Some(self.opts.load.n_ctx as usize),
            ..Limits::default()
        };
        let decider = tokio::task::spawn_blocking(move || {
            let tokenizer =
                ardana_registry::load_tokenizer(&model.tokenizer).map_err(|err| err.to_string())?;
            Decider::new(tokenizer, model.profile.clone())
                .map(|d| d.with_limits(limits))
                .map_err(|err| format!("model {}: {err}", model.name))
        })
        .await
        .map_err(|err| ApiError::Internal(format!("reading the tokenizer failed: {err}")))?
        .map_err(ApiError::Internal)?;
        let decider = Arc::new(decider);
        lock(&self.deciders).insert(entry.name.clone(), (entry, decider.clone()));
        Ok(decider)
    }

    /// Reserves `rows` queued rows, or refuses with 503.
    fn admit(&self, rows: usize) -> Result<Admission, ApiError> {
        let limit = self.opts.max_queued_rows;
        self.queued_rows
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |queued| {
                (queued + rows <= limit).then_some(queued + rows)
            })
            .map_err(|queued| {
                ApiError::Busy(format!(
                    "server busy: {queued} rows queued and this request adds {rows}, the limit is {limit}; retry later"
                ))
            })?;
        Ok(Admission {
            rows,
            queued: self.queued_rows.clone(),
        })
    }

    /// Queues `plan` on the worker of `model`, loading the model when needed, in its turn. A worker loaded from an
    /// older registry entry of the same name is unloaded.
    async fn enqueue(
        &self,
        model: &ResolvedModel,
        decider: &Arc<Decider>,
        plan: Plan,
        admission: Admission,
    ) -> Result<(Lease, oneshot::Receiver<Answer>), ApiError> {
        let turn = lock(&self.turns)
            .entry(model.name.clone())
            .or_default()
            .clone();
        let _turn = turn.lock().await;
        let listed = {
            let mut loaded = self.loaded.lock().await;
            loaded.retain(|w| w.alive() && (w.model.name != model.name || w.model == *model));
            loaded
                .iter()
                .find(|w| w.model == *model)
                .cloned()
                .map(Lease::new)
        };
        let lease = match listed {
            Some(lease) => lease,
            None => Lease::new(self.load(model, decider).await?),
        };
        let reply = lease.worker.submit(plan, admission)?;
        Ok((lease, reply))
    }

    /// Loads `model` once a model's place is free and lists its worker.
    async fn load(
        &self,
        model: &ResolvedModel,
        decider: &Arc<Decider>,
    ) -> Result<Arc<Worker>, ApiError> {
        let residence = self.reserve().await;
        let (runtimes, loading) = (self.runtimes.clone(), model.clone());
        let opts = self.opts.load;
        let weights = tokio::task::spawn_blocking(move || loading.load(&runtimes, &opts))
            .await
            .map_err(|err| {
                ApiError::Internal(format!("loading model {} failed: {err}", model.name))
            })?
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        let worker = Worker::start(
            model,
            decider.clone(),
            weights,
            residence,
            self.opts.max_queued_rows,
        )?;
        self.loaded.lock().await.push(worker.clone());
        self.residents.changed.notify_waiters();
        tokio::spawn(reap(
            Arc::downgrade(&self.loaded),
            Arc::downgrade(&worker),
            self.opts.keep_alive,
        ));
        Ok(worker)
    }

    /// A place among the models holding weights. When every place is taken by a listed model, the least recently used
    /// is unloaded; either way this waits until a model drops its weights (or another load lists its own).
    async fn reserve(&self) -> Residence {
        loop {
            let changed = self.residents.changed.notified();
            let mut changed = std::pin::pin!(changed);
            changed.as_mut().enable();
            if let Some(residence) = self.residents.try_reserve() {
                return residence;
            }
            {
                let mut loaded = self.loaded.lock().await;
                loaded.retain(|w| w.alive());
                // Models loading or still answering after an unload free (or take) a place on their own.
                if loaded.len() >= self.residents.count() {
                    let lru = (0..loaded.len()).min_by_key(|&i| loaded[i].last_used());
                    if let Some(lru) = lru {
                        loaded.remove(lru);
                    }
                }
            }
            changed.await;
        }
    }

    /// Unloads every model and waits for the workers to drop them; queued requests finish first.
    pub async fn shutdown(&self) {
        let workers = std::mem::take(&mut *self.loaded.lock().await);
        let threads: Vec<_> = workers.iter().filter_map(|w| w.take_thread()).collect();
        drop(workers);
        for thread in threads {
            // A worker that panicked already answered its request; there is nothing left to report.
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}

/// Pulls by name, each shared by every request for its name: the first request starts one detached task, later ones
/// wait for its outcome, and the pull is finished even when its requesters go away. A failed pull is forgotten, so
/// the next request tries again; a pulled one is forgotten too where something else holds what it pulled (the registry,
/// a library model), and kept where this is what holds it (a browser variant).
pub(crate) struct Pulls<T> {
    pulls: Arc<Mutex<HashMap<String, Outcome<T>>>>,
    keep: bool,
}

/// How a pull ended, once it has: what it pulled, or why not; every waiter watches it.
type Outcome<T> = watch::Receiver<Option<Result<T, String>>>;

impl<T: Clone + Send + Sync + 'static> Pulls<T> {
    /// Pulls forgotten once done.
    fn forgotten() -> Pulls<T> {
        Pulls {
            pulls: Arc::default(),
            keep: false,
        }
    }

    /// Pulls kept once pulled.
    fn kept() -> Pulls<T> {
        Pulls {
            pulls: Arc::default(),
            keep: true,
        }
    }

    /// What the pull of `name` pulled: the pull under way (or kept), else a new one that `pull` makes; `what` names
    /// the pull in an error.
    async fn get<F>(&self, name: &str, what: &str, pull: impl FnOnce() -> F) -> Result<T, ApiError>
    where
        F: Future<Output = Result<T, String>> + Send + 'static,
    {
        let mut outcome = {
            let mut pulls = lock(&self.pulls);
            match pulls.get(name) {
                Some(outcome) => outcome.clone(),
                None => {
                    let (done, outcome) = watch::channel(None);
                    pulls.insert(name.to_string(), outcome.clone());
                    tokio::spawn(finish(
                        pull(),
                        name.to_string(),
                        self.pulls.clone(),
                        self.keep,
                        done,
                    ));
                    outcome
                }
            }
        };
        let outcome = outcome
            .wait_for(Option::is_some)
            .await
            .map_err(|_| ApiError::Internal(format!("the pull of {what} stopped")))?
            .clone();
        match outcome {
            Some(Ok(pulled)) => Ok(pulled),
            Some(Err(err)) => Err(ApiError::Internal(err)),
            None => unreachable!("wait_for returns a sent outcome"),
        }
    }

    /// Forgets the pull of `name`, so the next request pulls again.
    fn forget(&self, name: &str) {
        lock(&self.pulls).remove(name);
    }
}

/// Runs the pull of `name`, forgets it unless it pulled what is kept here, then sends its outcome to everyone waiting.
async fn finish<T>(
    pull: impl Future<Output = Result<T, String>>,
    name: String,
    pulls: Arc<Mutex<HashMap<String, Outcome<T>>>>,
    keep: bool,
    done: watch::Sender<Option<Result<T, String>>>,
) {
    let outcome = pull.await;
    if !keep || outcome.is_err() {
        lock(&pulls).remove(&name);
    }
    // Every waiter may have gone away; the pull is done all the same.
    let _ = done.send(Some(outcome));
}

/// Locks `mutex`, whose data no panic leaves inconsistent.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The registry as last read, and the modification time of `models.toml` it was read at.
struct Catalog {
    registry: Arc<Registry>,
    modified: Option<SystemTime>,
}

impl Catalog {
    /// The registry, reread first when `models.toml` changed (or appeared, or went away) since the last read.
    fn current(&mut self) -> Result<Arc<Registry>, RegistryError> {
        let modified = self.registry.modified();
        if modified != self.modified {
            self.registry = Arc::new(Registry::open(&self.home())?);
            self.modified = modified;
        }
        Ok(self.registry.clone())
    }

    /// The Ardana home holding `models.toml`.
    fn home(&self) -> PathBuf {
        self.registry
            .path()
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default()
    }
}

/// Pulls the library model `name` into the registry in `home`, unless it is there by now.
async fn pull_into(
    name: &str,
    home: &std::path::Path,
    runtimes: Arc<Runtimes>,
) -> Result<(), String> {
    if Registry::open(home)
        .map_err(|err| err.to_string())?
        .entry(name)
        .is_ok()
    {
        return Ok(());
    }
    let pick = library()
        .find(name)
        .ok_or_else(|| format!("{name} is not a library model"))?;
    let size = pick
        .size()
        .map(|bytes| format!(", {}", ardana_api::human_size(bytes)))
        .unwrap_or_default();
    eprintln!("ardana serve: pulling {name} ({}{size})", pick.reference());
    let opts = PullOptions {
        progress: true,
        ..PullOptions::default()
    };
    let handle = tokio::runtime::Handle::current();
    let pulling = name.to_string();
    // The pull reads the tokenizer and the chat template synchronously: off the async workers.
    let model = tokio::task::spawn_blocking(move || {
        handle.block_on(ardana_registry::pull(&pulling, &opts, &runtimes))
    })
    .await
    .map_err(|err| format!("pulling {name} stopped: {err}"))?
    .map_err(|err| format!("pulling {name}: {err}"))?;
    // Read again right before writing, one pull at a time: `ardana pull` and other pulls may have added models
    // meanwhile.
    static WRITING: Mutex<()> = Mutex::new(());
    let _writing = lock(&WRITING);
    let mut registry = Registry::open(home).map_err(|err| err.to_string())?;
    registry.insert(model);
    registry.save().map_err(|err| err.to_string())?;
    eprintln!(
        "ardana serve: pulled {name} into {}",
        registry.path().display()
    );
    Ok(())
}

/// Queued rows reserved by one request, released when its job is decoded or discarded.
struct Admission {
    rows: usize,
    queued: Arc<AtomicUsize>,
}

impl Drop for Admission {
    fn drop(&mut self) {
        self.queued.fetch_sub(self.rows, Ordering::SeqCst);
    }
}

/// One request's hold on a worker: the model cannot go idle while a lease exists.
struct Lease {
    worker: Arc<Worker>,
}

impl Lease {
    fn new(worker: Arc<Worker>) -> Lease {
        worker.in_flight.fetch_add(1, Ordering::SeqCst);
        worker.touch();
        Lease { worker }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.worker.touch();
        self.worker.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Models holding weights: loading, loaded, or unloaded and still answering their queue.
struct Residents {
    count: AtomicUsize,
    limit: usize,
    /// Notified when a model drops its weights or a loaded one is listed.
    changed: Notify,
}

impl Residents {
    fn try_reserve(self: &Arc<Self>) -> Option<Residence> {
        self.count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.limit).then_some(n + 1)
            })
            .ok()
            .map(|_| Residence(self.clone()))
    }

    fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

/// One model's place among the [`Residents`], held from before its load until its weights are dropped.
struct Residence(Arc<Residents>);

impl Drop for Residence {
    fn drop(&mut self) {
        self.0.count.fetch_sub(1, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
}

type Answer = Result<SystemOneResponse, DecideError>;

struct Job {
    plan: Plan,
    reply: oneshot::Sender<Answer>,
    /// Released once the worker has decoded the job, or with the job when it is discarded.
    admission: Admission,
}

/// A loaded model: the channel to the thread that owns it.
struct Worker {
    /// The registry entry it was loaded from.
    model: ResolvedModel,
    jobs: mpsc::Sender<Job>,
    in_flight: AtomicUsize,
    last_used: Mutex<Instant>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Worker {
    /// Starts the thread that owns `model` and answers jobs in arrival order until every sender is gone, then drops
    /// the model and gives back its `residence`.
    fn start(
        model: &ResolvedModel,
        decider: Arc<Decider>,
        mut weights: Box<dyn LoadedModel>,
        residence: Residence,
        capacity: usize,
    ) -> Result<Arc<Worker>, ApiError> {
        let name = &model.name;
        let (jobs, mut rx) = mpsc::channel::<Job>(capacity);
        let thread = std::thread::Builder::new()
            .name(format!("ardana-model-{name}"))
            .spawn(move || {
                while let Some(Job {
                    plan,
                    reply,
                    admission,
                }) = rx.blocking_recv()
                {
                    let answer =
                        catch_unwind(AssertUnwindSafe(|| decider.run(weights.as_mut(), &plan)));
                    let panicked = answer.is_err();
                    let answer = answer.unwrap_or_else(|_| {
                        Err(DecideError::Runtime(anyhow::anyhow!(
                            "the model worker panicked"
                        )))
                    });
                    // The rows are done before the requester hears back, which may have gone away (the answer is
                    // then dropped).
                    drop(admission);
                    let _ = reply.send(answer);
                    if panicked {
                        break;
                    }
                }
                drop(weights);
                drop(residence);
            })
            .map_err(|err| {
                ApiError::Internal(format!("starting the worker of model {name}: {err}"))
            })?;
        Ok(Arc::new(Worker {
            model: model.clone(),
            jobs,
            in_flight: AtomicUsize::new(0),
            last_used: Mutex::new(Instant::now()),
            thread: Mutex::new(Some(thread)),
        }))
    }

    /// Queues `plan` with its admitted rows; the receiver gets the answer.
    fn submit(
        &self,
        plan: Plan,
        admission: Admission,
    ) -> Result<oneshot::Receiver<Answer>, ApiError> {
        let (reply, answer) = oneshot::channel();
        self.jobs
            .try_send(Job {
                plan,
                reply,
                admission,
            })
            .map_err(|err| match err {
                mpsc::error::TrySendError::Full(_) => ApiError::Busy(format!(
                    "server busy: the queue of model {} is full; retry later",
                    self.model.name
                )),
                mpsc::error::TrySendError::Closed(_) => {
                    ApiError::Internal(format!("the worker of model {} stopped", self.model.name))
                }
            })?;
        Ok(answer)
    }

    /// Whether the thread still takes jobs (it stops after a panic).
    fn alive(&self) -> bool {
        !self.jobs.is_closed()
    }

    fn last_used(&self) -> Instant {
        *lock(&self.last_used)
    }

    fn touch(&self) {
        *lock(&self.last_used) = Instant::now();
    }

    fn take_thread(&self) -> Option<std::thread::JoinHandle<()>> {
        lock(&self.thread).take()
    }
}

/// How often an unloader rechecks a model that is busy.
const BUSY_RECHECK: Duration = Duration::from_millis(100);

/// Unloads `worker` once it has been idle for `keep_alive`; ends when the worker is unloaded by anyone.
async fn reap(
    loaded: Weak<tokio::sync::Mutex<Vec<Arc<Worker>>>>,
    worker: Weak<Worker>,
    keep_alive: Duration,
) {
    loop {
        let Some(wait) = worker.upgrade().map(|w| {
            if w.in_flight.load(Ordering::SeqCst) > 0 {
                keep_alive.max(BUSY_RECHECK)
            } else {
                (w.last_used() + keep_alive).saturating_duration_since(Instant::now())
            }
        }) else {
            return;
        };
        tokio::time::sleep(wait).await;
        let (Some(loaded), Some(worker)) = (loaded.upgrade(), worker.upgrade()) else {
            return;
        };
        let mut loaded = loaded.lock().await;
        let Some(index) = loaded.iter().position(|w| Arc::ptr_eq(w, &worker)) else {
            return;
        };
        if worker.in_flight.load(Ordering::SeqCst) == 0
            && worker.last_used().elapsed() >= keep_alive
        {
            loaded.remove(index);
            return;
        }
    }
}
