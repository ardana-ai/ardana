//! [`Models`]: registry models behind the API, loaded on their first request.
//!
//! Every model reads its requests through a [`Decider`] (tokenizer and profile, built on the first request and kept),
//! so a request is validated and sized before any weights load. Loaded weights live on one worker thread per model
//! that decodes one request at a time from a FIFO channel. At most `max_loaded_models` hold weights at once, counting
//! models still loading and unloaded ones still answering their queue; loading one more evicts the least recently
//! used and waits until its weights are dropped, and a model idle for `keep_alive` is unloaded. Unloading closes the
//! worker's channel, so the worker finishes the requests it already has before it drops the model. A load holds only
//! its own model's turn, so other models and `/health` answer meanwhile. Admission counts the scoring rows of queued
//! and decoding requests over all models, until the worker is done with them, and refuses a request that would exceed
//! `max_queued_rows`.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use ardana_api::{ModelInfo, ModelsResponse, SystemOneRequest, SystemOneResponse};
use ardana_core::{DecideError, Decider, Limits, LoadOptions, LoadedModel, Plan, Runtimes};
use ardana_registry::{Registry, RegistryError, ResolvedModel};
use serde_json::{Map, Value, json};
use tokio::sync::{Notify, mpsc, oneshot};

use crate::error::{ApiError, ValidationItem};

/// Every `jev-*` model name, `jev-latest` included, means the default model (Q7).
const JEV_PREFIX: &str = "jev-";

/// How [`Models`] loads and schedules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOptions {
    /// The model `jev-*` names and requests without a model resolve to; `None` is the first registry entry.
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
    registry: Registry,
    runtimes: Arc<Runtimes>,
    opts: ModelOptions,
    /// The resolved default model; `None` only for an empty registry.
    default: Option<String>,
    deciders: Mutex<HashMap<String, Arc<Decider>>>,
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
            .field("names", &self.registry.names())
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
                registry
                    .entry(name)
                    .map_err(ModelsError::DefaultModel)?
                    .name
                    .clone(),
            ),
            None => registry.names().into_iter().next(),
        };
        let residents = Arc::new(Residents {
            count: AtomicUsize::new(0),
            limit: opts.max_loaded_models,
            changed: Notify::new(),
        });
        Ok(Models {
            registry,
            runtimes: Arc::new(runtimes),
            opts,
            default,
            deciders: Mutex::new(HashMap::new()),
            turns: Mutex::new(HashMap::new()),
            loaded: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            residents,
            queued_rows: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// The registry name a request's `model` means: `jev-*` and no model are the default model (Q7).
    pub fn resolve(&self, requested: Option<&str>) -> Result<String, ApiError> {
        let name = match requested {
            None => self.default.as_deref(),
            Some(name) if name.starts_with(JEV_PREFIX) => self.default.as_deref(),
            Some(name) => Some(name),
        };
        let name = name.unwrap_or(requested.unwrap_or("jev-latest"));
        self.registry
            .entry(name)
            .map(|model| model.name.clone())
            .map_err(|err| ApiError::UnknownModel(err.to_string()))
    }

    /// `GET /v1/models`: every registry entry, described by its source reference and dated by its profile's release
    /// date, else the day it was pulled.
    pub fn list(&self) -> ModelsResponse {
        ModelsResponse {
            models: self
                .registry
                .entries()
                .iter()
                .map(|m| ModelInfo {
                    name: m.name.clone(),
                    description: m.source.clone(),
                    release_date: m
                        .profile
                        .release_date
                        .clone()
                        .unwrap_or_else(|| m.pulled_at.clone()),
                })
                .collect(),
        }
    }

    /// `GET /health`: `status` plus the loaded models (most recently used first) and every model's temperatures.
    pub async fn health(&self) -> Value {
        let temperatures: Map<String, Value> = self
            .registry
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
        json!({
            "status": "ok",
            "x_loaded": self.loaded().await,
            "x_temperature_by_type": temperatures,
        })
    }

    /// The loaded models, most recently used first.
    pub async fn loaded(&self) -> Vec<String> {
        let mut loaded: Vec<(Instant, String)> = self
            .loaded
            .lock()
            .await
            .iter()
            .map(|w| (w.last_used(), w.name.clone()))
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
        let name = self.resolve(req.model.as_deref())?;
        let decider = self.decider(&name).await?;
        let planner = decider.clone();
        let plan = tokio::task::spawn_blocking(move || planner.plan(&req))
            .await
            .map_err(|err| ApiError::Internal(format!("planning the request failed: {err}")))?
            .map_err(decide_error)?;
        let admission = self.admit(plan.rows())?;
        let (_lease, reply) = self.enqueue(&name, &decider, plan, admission).await?;
        let answer = reply
            .await
            .map_err(|_| ApiError::Internal(format!("the worker of model {name} stopped")))?;
        answer.map_err(decide_error)
    }

    /// The model's reader, built on its first request.
    async fn decider(&self, name: &str) -> Result<Arc<Decider>, ApiError> {
        if let Some(decider) = self.lock_deciders().get(name) {
            return Ok(decider.clone());
        }
        let model = self.resolved(name)?;
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
        Ok(self
            .lock_deciders()
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(decider))
            .clone())
    }

    fn lock_deciders(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Decider>>> {
        self.deciders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn resolved(&self, name: &str) -> Result<ResolvedModel, ApiError> {
        self.registry
            .resolve(name)
            .map_err(|err| ApiError::Internal(err.to_string()))
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

    /// Queues `plan` on the worker of `name`, loading the model when needed, in `name`'s turn.
    async fn enqueue(
        &self,
        name: &str,
        decider: &Arc<Decider>,
        plan: Plan,
        admission: Admission,
    ) -> Result<(Lease, oneshot::Receiver<Answer>), ApiError> {
        let turn = self
            .turns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(name.to_string())
            .or_default()
            .clone();
        let _turn = turn.lock().await;
        let listed = {
            let mut loaded = self.loaded.lock().await;
            loaded.retain(|w| w.alive());
            loaded
                .iter()
                .find(|w| w.name == name)
                .cloned()
                .map(Lease::new)
        };
        let lease = match listed {
            Some(lease) => lease,
            None => Lease::new(self.load(name, decider).await?),
        };
        let reply = lease.worker.submit(plan, admission)?;
        Ok((lease, reply))
    }

    /// Loads `name` once a model's place is free and lists its worker.
    async fn load(&self, name: &str, decider: &Arc<Decider>) -> Result<Arc<Worker>, ApiError> {
        let residence = self.reserve().await;
        let model = self.resolved(name)?;
        let runtimes = self.runtimes.clone();
        let opts = self.opts.load;
        let weights = tokio::task::spawn_blocking(move || model.load(&runtimes, &opts))
            .await
            .map_err(|err| ApiError::Internal(format!("loading model {name} failed: {err}")))?
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        let worker = Worker::start(
            name,
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

/// A decide failure as an API error: bad input is 422, too much input 413, a runtime failure 500.
fn decide_error(err: DecideError) -> ApiError {
    match err {
        DecideError::Invalid { loc, msg } => ApiError::Validation(vec![ValidationItem::new(
            loc.into_iter().map(Value::String).collect(),
            msg,
            "value_error",
        )]),
        DecideError::Capacity(msg) => ApiError::TooLarge(msg),
        DecideError::Runtime(err) => ApiError::Internal(format!("{err:#}")),
    }
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
    name: String,
    jobs: mpsc::Sender<Job>,
    in_flight: AtomicUsize,
    last_used: Mutex<Instant>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Worker {
    /// Starts the thread that owns `model` and answers jobs in arrival order until every sender is gone, then drops
    /// the model and gives back its `residence`.
    fn start(
        name: &str,
        decider: Arc<Decider>,
        mut model: Box<dyn LoadedModel>,
        residence: Residence,
        capacity: usize,
    ) -> Result<Arc<Worker>, ApiError> {
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
                        catch_unwind(AssertUnwindSafe(|| decider.run(model.as_mut(), &plan)));
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
                drop(model);
                drop(residence);
            })
            .map_err(|err| {
                ApiError::Internal(format!("starting the worker of model {name}: {err}"))
            })?;
        Ok(Arc::new(Worker {
            name: name.to_string(),
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
                    self.name
                )),
                mpsc::error::TrySendError::Closed(_) => {
                    ApiError::Internal(format!("the worker of model {} stopped", self.name))
                }
            })?;
        Ok(answer)
    }

    /// Whether the thread still takes jobs (it stops after a panic).
    fn alive(&self) -> bool {
        !self.jobs.is_closed()
    }

    fn last_used(&self) -> Instant {
        *self
            .last_used
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn touch(&self) {
        *self
            .last_used
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
    }

    fn take_thread(&self) -> Option<std::thread::JoinHandle<()>> {
        self.thread
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
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
