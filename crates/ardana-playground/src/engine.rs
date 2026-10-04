//! The in-tab engine: a library model's browser variant, run in this tab (Q9). The server pulls the variant once and
//! serves its files from its own origin (`/v1/browser/<name>/*`), and the standalone build fetches them from their
//! Hugging Face repository ([`ApiClient::browser_file`]); the tab downloads them once into its Cache Storage
//! (where the page has one: only a secure context does). Two modules, imported on an "In browser" pick and never
//! before, run it: the engine module (`crates/ardana-engine`, built by trunk into `engine/`) plans the request and
//! reads the answers out with `ardana-core` as the server does, and onnxruntime-web (`engine.js`) decodes each row on
//! WebGPU, else WASM; this page's own wasm carries neither. A run yields the [`Exchange`] a server run would: the exact
//! body sent, and the status and body text the API answers that request with, refusals included. A run can be stopped
//! ([`stop`]): its downloads end where they are.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use ardana_api::human_size;
use js_sys::{Float32Array, Uint8Array, Uint32Array};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, AbortSignal, Cache, CacheStorage, Response};

use crate::api::{ApiClient, Backend, Exchange, Failed, Place, Unanswered, js_error, now};

// `engine.js`, which trunk copies beside the engine module (`index.html`): a file of its own rather than a wasm-bindgen
// snippet, so `engine/` holds every script of the in-tab engine, whose names never change. The path is relative to this
// page's own module, beside `engine/` wherever the page is served.
#[wasm_bindgen(raw_module = "./engine/engine.js")]
extern "C" {
    #[wasm_bindgen(js_name = hasWebGpu)]
    fn has_webgpu() -> bool;

    /// Starts importing the engine module and onnxruntime-web, ahead of a run in this tab.
    pub fn prepare();

    /// The engine module, imported and started on first use; a failed import is imported again by the next call.
    #[wasm_bindgen(catch)]
    async fn engine() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn runtime(device: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_name = createSession, catch)]
    async fn create_session(
        ort: &JsValue,
        model: &Uint8Array,
        data: &Uint8Array,
        device: &str,
        signal: &AbortSignal,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn decode(
        session: &JsValue,
        ids: &Uint32Array,
        labels: &Uint32Array,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn release(session: &JsValue) -> Result<JsValue, JsValue>;
}

// The engine module's exports (`crates/ardana-engine`), reached through the module object `engine` resolves to.
#[wasm_bindgen]
extern "C" {
    /// The engine module, started.
    type Engine;

    /// Reads the profile the server sent for a browser variant.
    #[wasm_bindgen(method, catch)]
    fn profile(this: &Engine, json: &str) -> Result<Profile, JsValue>;

    /// The reader of the browser variant of `name`, from its tokenizer's bytes and its profile.
    #[wasm_bindgen(method, catch)]
    fn reader(
        this: &Engine,
        name: &str,
        tokenizer: &Uint8Array,
        profile: &Profile,
    ) -> Result<Reader, JsValue>;

    /// A browser variant's profile, read by the engine module.
    type Profile;

    /// A browser variant's reader: its tokenizer and profile.
    type Reader;

    /// Plans a `/v1/systemone` body as the server plans it.
    #[wasm_bindgen(method, catch)]
    fn plan(this: &Reader, body: &str) -> Result<Run, JsValue>;

    /// A request the engine module planned, decoded in this tab row by row.
    type Run;

    /// Whether the request is answered without a decode (refused as the server refuses it).
    #[wasm_bindgen(method)]
    fn refused(this: &Run) -> bool;

    #[wasm_bindgen(method)]
    fn rows(this: &Run) -> u32;

    #[wasm_bindgen(method)]
    fn ids(this: &Run, row: u32) -> Uint32Array;

    #[wasm_bindgen(method)]
    fn labels(this: &Run, row: u32) -> Uint32Array;

    /// The logits of the next row, at its labels.
    #[wasm_bindgen(method)]
    fn push(this: &Run, logits: &Float32Array);

    /// A row did not decode: the run answers as the server answers a runtime that failed.
    #[wasm_bindgen(method)]
    fn fail(this: &Run, reason: &str);

    /// The status the API answers the request with.
    #[wasm_bindgen(method)]
    fn status(this: &Run) -> u16;

    /// The body text the API answers the request with.
    #[wasm_bindgen(method)]
    fn body(this: &Run) -> String;

    /// An object the engine module handed out.
    type Exported;

    /// Frees the object's memory, which is the engine module's.
    #[wasm_bindgen(method)]
    fn free(this: &Exported);
}

/// An onnxruntime-web session released once dropped, in the background: a session no later run keeps.
struct Released(JsValue);

impl Drop for Released {
    fn drop(&mut self) {
        let session = self.0.clone();
        leptos::task::spawn_local(async move {
            // A session that fails to free itself leaves its memory to the page; nothing else depends on it.
            let _ = release(&session).await;
        });
    }
}

/// An object of the engine module, freed once dropped: its memory is that module's, which this page's garbage
/// collector reclaims late, if at all.
struct Freed<T: JsCast>(T);

impl<T: JsCast> Drop for Freed<T> {
    fn drop(&mut self) {
        self.0.unchecked_ref::<Exported>().free();
    }
}

impl<T: JsCast> Deref for Freed<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// The files of a browser variant, in the order a run needs them: the tokenizer plans the request before the weights
/// are fetched, so a refused request downloads only the tokenizer.
const TOKENIZER: &str = "tokenizer.json";
const GRAPH: &str = "model.onnx";
const WEIGHTS: &str = "model.onnx.data";

/// The Cache Storage caches of browser variants: `ardana-browser <model> <version>`, one per model and version of
/// its files.
const CACHE_PREFIX: &str = "ardana-browser";

/// How many bytes of a file a download keeps at a time as they arrive: a download that stops resumes after its last
/// whole part.
const PART: u64 = 4 << 20;

/// Where a run in the tab is, for the banner and the Run key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The tab has asked the server for the model's browser files, which the server holds: it answers at once.
    Asking,
    /// The server is pulling the model's browser files: it did not hold them when the tab asked
    /// (`x_browser_pulled`).
    Pulling,
    /// The tab is downloading them: the bytes it has, kept or downloaded, of the bytes all three files hold.
    Downloading { done: u64, total: u64 },
    /// The tab has every file and is starting the model: reading the files it kept, loading the tokenizer, starting
    /// onnxruntime-web.
    Starting,
    /// The rows are decoding.
    Running(Backend),
}

/// The model this tab has loaded, kept between runs; one at a time.
struct Loaded {
    name: String,
    version: String,
    reader: Rc<Freed<Reader>>,
    /// The bytes of the tokenizer the reader was read from, which count as had when the next run downloads the rest.
    tokenizer_bytes: u64,
    /// The onnxruntime-web session, once a run needed one.
    session: Option<(JsValue, Backend)>,
}

thread_local! {
    static LOADED: RefCell<Option<Loaded>> = const { RefCell::new(None) };
    /// The run in flight, which [`stop`] ends, and where it is.
    static RUNNING: RefCell<Option<(AbortController, RwSignal<Option<Stage>>)>> = const { RefCell::new(None) };
    /// Whether the last run [`stop`] ended was downloading: the next run resumes that download.
    static STOPPED_DOWNLOAD: Cell<bool> = const { Cell::new(false) };
}

/// Runs `body`, a `/v1/systemone` request, on the browser variant of `name` (`total`: its files' bytes, `x_browser`;
/// `pulled`: whether the server held them when the page last listed its models, `x_browser_pulled`), reporting where it
/// is on `stage`.
pub async fn run(
    client: &ApiClient,
    name: &str,
    total: u64,
    pulled: bool,
    body: String,
    stage: RwSignal<Option<Stage>>,
) -> Exchange {
    let mut place = Place::Tab(None);
    stage.set(Some(Stage::Asking));
    // The server answers at once for the browser files it holds, and pulls the others from the Hub first: the tab says
    // it pulls only when the server, asked now (it may have pulled them since the page listed its models), does not
    // hold them.
    if !pulled && !held(client, name).await {
        stage.set(Some(Stage::Pulling));
    }
    let outcome = match AbortController::new() {
        Ok(controller) => {
            let signal = controller.signal();
            RUNNING.set(Some((controller, stage)));
            let answered = answer(client, name, total, &body, stage, &signal, &mut place).await;
            RUNNING.set(None);
            answered
        }
        Err(err) => Err(Unanswered {
            reason: format!("this browser cannot stop a download: {}", js_error(err)),
            advice: Some(format!(
                "This browser cannot run {name} in a tab. Run it with the ardana CLI instead."
            )),
        }),
    };
    stage.set(None);
    let (response, latency_ms) = match outcome {
        Ok((status, text, latency)) => (Ok((status, text)), latency),
        Err(failure) => (Err(failure), 0.0),
    };
    Exchange {
        request: body,
        response,
        latency_ms,
        place,
    }
}

/// Stops the run in this tab: a download ends where it is, and a session it is starting is released once started.
pub fn stop() {
    let running = RUNNING.take();
    STOPPED_DOWNLOAD.set(running.as_ref().is_some_and(|(_, stage)| {
        matches!(
            stage.try_get_untracked().flatten(),
            Some(Stage::Downloading { .. })
        )
    }));
    if let Some((controller, _)) = running {
        controller.abort();
    }
}

/// The status and body text the API answers `body` with, and the milliseconds the decision took in the tab.
async fn answer(
    client: &ApiClient,
    name: &str,
    total: u64,
    body: &str,
    stage: RwSignal<Option<Stage>>,
    signal: &AbortSignal,
    place: &mut Place,
) -> Result<(u16, String, f64), Unanswered> {
    let profile = client.browser_profile(name, signal).await;
    let (profile, version) = profile.map_err(|failed| match failed {
        Failed::Connection(reason) => Unanswered {
            reason: format!("asking the server for the browser files of {name}: {reason}"),
            advice: Some(
                "This tab could not reach the server. Run again once the connection is back."
                    .to_string(),
            ),
        },
        Failed::Refused(reason) => refused(name, reason),
    })?;
    let engine = engine_module().await?;
    let profile = Freed(
        engine
            .profile(&profile)
            .map_err(|err| unreadable(name, js_error(err)))?,
    );

    let mut files = Files::open(client, name, &version, total, stage, signal).await;
    let kept_reader = loaded(name, &version, |loaded| {
        (loaded.reader.clone(), loaded.tokenizer_bytes)
    });
    let kept_session = loaded(name, &version, |loaded| loaded.session.clone()).flatten();
    // The files this run reads: the tokenizer unless its reader is loaded, the graph and its weights unless a session
    // is. One stage spans them: downloading while any is not kept, else starting.
    let mut needed = Vec::new();
    if kept_reader.is_none() {
        needed.push(TOKENIZER);
    }
    if kept_session.is_none() {
        needed.extend([GRAPH, WEIGHTS]);
    }
    let downloads = !files.keeps(&needed).await;
    if let Some((_, tokenizer_bytes)) = &kept_reader {
        files.done += tokenizer_bytes;
    }
    stage.set(Some(if downloads {
        Stage::Downloading {
            done: files.done.min(total),
            total,
        }
    } else {
        Stage::Starting
    }));

    let reader = match kept_reader {
        Some((reader, _)) => reader,
        None => {
            let bytes = files.get(TOKENIZER).await?;
            let reader = engine
                .reader(name, &bytes, &profile)
                .map_err(|err| unreadable(name, js_error(err)))?;
            let reader = Rc::new(Freed(reader));
            replace(Loaded {
                name: name.to_string(),
                version: version.clone(),
                reader: reader.clone(),
                tokenizer_bytes: u64::from(bytes.length()),
                session: None,
            })
            .await;
            reader
        }
    };

    let started = now();
    let run = Freed(
        reader
            .plan(body)
            .map_err(|err| unreadable(name, js_error(err)))?,
    );
    if run.refused() {
        return Ok((run.status(), run.body(), now() - started));
    }
    let planning = now() - started;

    // A session no later run keeps is released once this run is over, answered or stopped.
    let mut _released = None;
    let (session, backend) = match kept_session {
        Some(session) => session,
        None => {
            let graph = files.get(GRAPH).await?;
            let weights = files.get(WEIGHTS).await?;
            stage.set(Some(Stage::Starting));
            let (session, backend, kept) = start(name, &graph, &weights, signal).await?;
            if kept {
                LOADED.with_borrow_mut(|loaded| {
                    if let Some(loaded) = loaded
                        .as_mut()
                        .filter(|loaded| loaded.name == name && loaded.version == version)
                    {
                        loaded.session = Some((session.clone(), backend));
                    }
                });
            } else {
                _released = Some(Released(session.clone()));
            }
            (session, backend)
        }
    };
    files.forget_older().await;
    *place = Place::Tab(Some(backend));

    stage.set(Some(Stage::Running(backend)));
    let started = now();
    decide(&run, &session).await;
    let latency = planning + (now() - started);
    Ok((run.status(), run.body(), latency))
}

/// Whether the server holds the browser files of `name` whole, as its model list says now.
async fn held(client: &ApiClient, name: &str) -> bool {
    client.models().await.is_ok_and(|list| {
        list.models
            .iter()
            .any(|model| model.name == name && model.x_browser_pulled)
    })
}

/// The engine module, started; a failed import fails this run, and the next run imports the module again.
async fn engine_module() -> Result<Engine, Unanswered> {
    let engine = engine().await.map_err(|err| Unanswered {
        reason: format!("importing Ardana's engine: {}", js_error(err)),
        advice: Some(
            "Ardana's engine did not load into this tab. Run again once the connection is back."
                .to_string(),
        ),
    })?;
    Ok(engine.unchecked_into())
}

/// The server refused the browser files of `name`: its message is the reason.
fn refused(name: &str, reason: String) -> Unanswered {
    Unanswered {
        reason,
        advice: Some(format!(
            "The server could not provide the browser files of {name}. Try again later."
        )),
    }
}

/// The tab cannot read what the server sent for `name`: a page older than its server reads it after a reload.
fn unreadable(name: &str, reason: String) -> Unanswered {
    Unanswered {
        reason,
        advice: Some(format!(
            "This tab could not read the files of {name}. Reload the page, then run again."
        )),
    }
}

/// A value of the loaded model, when it is `name` at `version`.
fn loaded<T>(name: &str, version: &str, get: impl FnOnce(&Loaded) -> T) -> Option<T> {
    LOADED.with_borrow(|loaded| {
        loaded
            .as_ref()
            .filter(|loaded| loaded.name == name && loaded.version == version)
            .map(get)
    })
}

/// Releases the model this tab has loaded, as picking a server row does: its onnxruntime-web session (the weights, on
/// the GPU or in onnxruntime-web's memory) and its reader's memory in the engine module. A later run in the tab loads
/// it again from the files this browser keeps.
pub fn unload() {
    if let Some(loaded) = LOADED.take()
        && let Some((session, _)) = loaded.session
    {
        drop(Released(session));
    }
}

/// Makes `next` the loaded model, freeing the session of the one before it.
async fn replace(next: Loaded) {
    let before = LOADED.with_borrow_mut(|loaded| loaded.replace(next));
    if let Some((session, _)) = before.and_then(|before| before.session) {
        // A session that fails to free itself leaves its memory to the page; nothing else depends on it.
        let _ = release(&session).await;
    }
}

/// A session over the graph and its weights: on WebGPU when the browser has it, else (or when WebGPU cannot run it)
/// on WASM; and whether later runs keep it. A build that did not load (its bundle or its WASM module could not be
/// fetched) is imported again by the next run, so a session on WASM that stands in for WebGPU's answers this run
/// alone; a build that loaded and could not start the model keeps that failure until the page is reloaded, and the
/// WASM session that stands in for it is kept.
async fn start(
    name: &str,
    graph: &Uint8Array,
    weights: &Uint8Array,
    signal: &AbortSignal,
) -> Result<(JsValue, Backend, bool), Unanswered> {
    let backends: &[Backend] = if has_webgpu() {
        &[Backend::WebGpu, Backend::Wasm]
    } else {
        &[Backend::Wasm]
    };
    let mut reasons = Vec::new();
    let mut unloaded = false;
    for &backend in backends {
        let ort = match runtime(backend.provider()).await {
            Ok(ort) => ort,
            Err(err) => {
                unloaded = true;
                reasons.push(format!("on {}: {}", backend.name(), js_error(err)));
                continue;
            }
        };
        match create_session(&ort, graph, weights, backend.provider(), signal).await {
            Ok(session) => return Ok((session, backend, !unloaded)),
            Err(err) => {
                // `engine.js` forgot the build: its WASM module could not be fetched.
                unloaded |= err
                    .dyn_ref::<js_sys::Error>()
                    .is_some_and(|err| err.name() == "NotLoaded");
                reasons.push(format!("on {}: {}", backend.name(), js_error(err)));
            }
        }
    }
    let advice = if unloaded {
        "onnxruntime-web did not load into this tab. Run again once the connection is back."
            .to_string()
    } else {
        let names: Vec<&str> = backends.iter().map(|backend| backend.name()).collect();
        format!(
            "This browser could not start {name} on {}. Reload the page, then run again.",
            names.join(" or ")
        )
    };
    Err(Unanswered {
        reason: format!(
            "onnxruntime-web could not start {name} in this tab: {}",
            reasons.join("; ")
        ),
        advice: Some(advice),
    })
}

/// Decodes every row of `run` on the session, handing each row's logits back to it; a row that does not decode ends
/// the run.
async fn decide(run: &Run, session: &JsValue) {
    for row in 0..run.rows() {
        match decode(session, &run.ids(row), &run.labels(row)).await {
            Ok(logits) => run.push(logits.unchecked_ref()),
            Err(err) => return run.fail(&format!("onnxruntime-web: {}", js_error(err))),
        }
    }
}

/// Whether a run of the browser variant of `name` in this tab downloads nothing: this tab has the model loaded, or
/// this browser keeps every file of a version of it (the version the server serves, unless its copy changed since).
pub async fn kept(name: &str) -> bool {
    let loaded = LOADED.with_borrow(|loaded| {
        loaded
            .as_ref()
            .is_some_and(|loaded| loaded.name == name && loaded.session.is_some())
    });
    if loaded {
        return true;
    }
    let Some(caches) = caches() else {
        return false;
    };
    let Ok(names) = JsFuture::from(caches.keys()).await else {
        return false;
    };
    let model = cache_name(name, "");
    for cache in js_sys::Array::from(&names)
        .iter()
        .filter_map(|name| name.as_string())
        .filter(|cache| cache.starts_with(&model))
    {
        let Ok(cache) = JsFuture::from(caches.open(&cache)).await else {
            continue;
        };
        if holds(&cache.unchecked_into(), name, &[TOKENIZER, GRAPH, WEIGHTS]).await {
            return true;
        }
    }
    false
}

/// Whether this page can keep the files a run downloads: a page without Cache Storage (not a secure context, or a
/// browser that withholds it) downloads them again on each visit.
pub fn keeps_files() -> bool {
    caches().is_some()
}

/// What the run status says of a run in this tab that was stopped; stopped while it downloaded, what becomes of the
/// download: the next run resumes it where this page keeps what it received, and starts it over where it cannot.
pub fn stopped() -> String {
    let stopped = "Stopped, not answered";
    if !STOPPED_DOWNLOAD.get() {
        stopped.to_string()
    } else if keeps_files() {
        format!("{stopped}. The next run resumes the download.")
    } else {
        format!(
            "{stopped}. The next run starts the download over: a page without HTTPS keeps no files."
        )
    }
}

/// Whether `cache` holds every one of `files` of `name`'s browser variant.
async fn holds(cache: &Cache, name: &str, files: &[&str]) -> bool {
    for file in files {
        let found = JsFuture::from(cache.match_with_str(&file_url(name, file))).await;
        if !found.is_ok_and(|found| found.is_instance_of::<Response>()) {
            return false;
        }
    }
    true
}

/// A browser variant's files at one version: from this browser's Cache Storage when it kept them, else downloaded
/// with progress and kept. A download keeps what it has received in parts as it goes, so one that stops (the connection
/// drops, Stop, a reload) resumes after its last whole part; a file that arrived whole is kept once, without its parts.
struct Files<'a> {
    client: &'a ApiClient,
    name: &'a str,
    version: &'a str,
    stage: RwSignal<Option<Stage>>,
    signal: &'a AbortSignal,
    /// The cache of this version; `None` where the page has no Cache Storage.
    cache: Option<Cache>,
    /// The bytes of the files this run has, kept or downloaded, of `total`.
    done: u64,
    total: u64,
}

/// A part of a file kept while it downloaded: its bytes `start` to `end` (both included) of the `length` the whole file
/// has, kept under `url` (`<file's url>?bytes=<start>-<end>/<length>`).
struct Part {
    start: u64,
    end: u64,
    length: u64,
    url: String,
}

/// What this browser kept of a file whose download stopped: its first `had` bytes, from its parts, in an array as long
/// as the whole file.
struct Prefix {
    bytes: Uint8Array,
    had: u64,
}

impl<'a> Files<'a> {
    async fn open(
        client: &'a ApiClient,
        name: &'a str,
        version: &'a str,
        total: u64,
        stage: RwSignal<Option<Stage>>,
        signal: &'a AbortSignal,
    ) -> Files<'a> {
        let cache = match caches() {
            Some(caches) => JsFuture::from(caches.open(&cache_name(name, version)))
                .await
                .ok()
                .map(JsCast::unchecked_into),
            None => None,
        };
        Files {
            client,
            name,
            version,
            stage,
            signal,
            cache,
            done: 0,
            total,
        }
    }

    /// Whether this browser keeps every one of `files` at this version.
    async fn keeps(&self, files: &[&str]) -> bool {
        match &self.cache {
            Some(cache) => holds(cache, self.name, files).await,
            None => files.is_empty(),
        }
    }

    /// The bytes of `file`: kept by this browser, else downloaded (the banner shows how far) and kept. A download that
    /// stopped before resumes after the parts it kept: the server sends the rest of the file, or the whole of it when
    /// its copy is no longer this version.
    async fn get(&mut self, file: &str) -> Result<Uint8Array, Unanswered> {
        let url = file_url(self.name, file);
        if let Some(bytes) = self.kept(&url).await {
            self.done += u64::from(bytes.length());
            // While the run downloads, the bar counts what it already has.
            if matches!(self.stage.get_untracked(), Some(Stage::Downloading { .. })) {
                self.show(self.done);
            }
            return Ok(bytes);
        }
        let parts = self.parts(file).await;
        let prefix = self.prefix(&parts).await;
        let from = prefix.as_ref().map_or(0, |prefix| prefix.had);
        let base = self.done;
        let mut download = self
            .client
            .browser_file(self.name, file, from, self.version, self.signal)
            .await
            .map_err(|failed| self.fault(file, failed, base))?;
        let bytes = match prefix {
            Some(prefix) if download.start > 0 => {
                // The rest of the version the parts came from, so a file of their length.
                if u64::from(prefix.bytes.length()) != download.length {
                    self.forget(&parts).await;
                    return Err(refused(
                        self.name,
                        format!(
                            "{url} sent the rest of {} bytes, after parts of a file of {}",
                            download.length,
                            prefix.bytes.length()
                        ),
                    ));
                }
                prefix.bytes
            }
            _ => {
                // The server sends the whole file: whatever was kept of it (a copy it no longer serves) goes.
                self.forget(&parts).await;
                let length = u32::try_from(download.length).map_err(|_| {
                    refused(
                        self.name,
                        format!("{url} holds {} bytes, more than a tab can", download.length),
                    )
                })?;
                Uint8Array::new_with_length(length)
            }
        };
        // The bytes before `start` are kept already, and count as had.
        let mut at = download.start;
        let mut shown = base + at;
        self.show(shown);
        // Where the part being received starts; parts stop being kept once the browser refuses one.
        let mut part = Some(at);
        while let Some(chunk) = download
            .next()
            .await
            .map_err(|failed| self.fault(file, failed, base + at))?
        {
            // The whole file fits the array (`download.length` fits a `u32`), and every chunk fits the file.
            bytes.set(&chunk, at as u32);
            at += u64::from(chunk.length());
            // Redrawn every half percent: often enough to move, not on every chunk.
            if base + at - shown >= self.total / 200 || base + at >= self.total {
                shown = base + at;
                self.show(shown);
            }
            if let Some(start) = part.filter(|start| at - start >= PART && at < download.length) {
                let url = part_url(self.name, file, start, at - 1, download.length);
                let kept = self
                    .keep(&url, &bytes.subarray(start as u32, at as u32))
                    .await;
                part = kept.then_some(at);
            }
        }
        self.done = base + download.length;
        // Kept once: whole, without the parts it came in; a browser that refuses the whole file keeps the parts, and
        // the next run downloads the rest after them.
        if self.keep(&url, &bytes).await {
            self.forget(&self.parts(file).await).await;
        }
        Ok(bytes)
    }

    /// The bar at `done` bytes of the run's files.
    fn show(&self, done: u64) {
        self.stage.set(Some(Stage::Downloading {
            done: done.min(self.total),
            total: self.total,
        }));
    }

    /// Why the download of `file` stopped once the run had `had` bytes of its files, and what to do next.
    fn fault(&self, file: &str, failed: Failed, had: u64) -> Unanswered {
        match failed {
            Failed::Connection(reason) => Unanswered {
                reason: format!("downloading {file} of {}: {reason}", self.name),
                advice: Some(format!(
                    "The download of {} stopped at {} of {}. Run again once the connection is back {}",
                    self.name,
                    human_size(had.min(self.total)),
                    human_size(self.total),
                    if keeps_files() {
                        "to resume it."
                    } else {
                        "to start it over: a page without HTTPS keeps no files."
                    }
                )),
            },
            Failed::Refused(reason) => refused(self.name, reason),
        }
    }

    async fn kept(&self, url: &str) -> Option<Uint8Array> {
        let cache = self.cache.as_ref()?;
        let found = JsFuture::from(cache.match_with_str(url)).await.ok()?;
        let response: Response = found.dyn_into().ok()?;
        let buffer = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
        Some(Uint8Array::new(&buffer))
    }

    /// Keeps `bytes` under `url` for a later run, and says whether this browser kept them. A browser that refuses (its
    /// storage is full or off) downloads them again next time; the console says why.
    async fn keep(&self, url: &str, bytes: &Uint8Array) -> bool {
        let Some(cache) = &self.cache else {
            return false;
        };
        let kept = match Response::new_with_opt_buffer_source(Some(bytes)) {
            Ok(response) => JsFuture::from(cache.put_with_str(url, &response))
                .await
                .map(drop),
            Err(err) => Err(err),
        };
        if let Err(err) = &kept {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "Ardana: this browser did not keep {url}: {}",
                js_error(err.clone())
            )));
        }
        kept.is_ok()
    }

    /// The parts this version's cache keeps of `file`, read from their URLs.
    async fn parts(&self, file: &str) -> Vec<Part> {
        let Some(cache) = &self.cache else {
            return Vec::new();
        };
        let Ok(keys) = JsFuture::from(cache.keys()).await else {
            return Vec::new();
        };
        let named = format!("{}?bytes=", file_url(self.name, file));
        let mut parts: Vec<Part> = js_sys::Array::from(&keys)
            .iter()
            .filter_map(|key| {
                let url = key.unchecked_into::<web_sys::Request>().url();
                let range = &url[url.find(&named)? + named.len()..];
                let (span, length) = range.split_once('/')?;
                let (start, end) = span.split_once('-')?;
                Some(Part {
                    start: start.parse().ok()?,
                    end: end.parse().ok()?,
                    length: length.parse().ok()?,
                    url: url.clone(),
                })
            })
            .collect();
        parts.sort_by_key(|part| part.start);
        parts
    }

    /// The bytes `parts` hold from the file's first byte on, one part after the other; none without a first part.
    async fn prefix(&self, parts: &[Part]) -> Option<Prefix> {
        let length = parts.first().filter(|part| part.start == 0)?.length;
        let bytes = Uint8Array::new_with_length(u32::try_from(length).ok()?);
        let mut had = 0;
        for part in parts.iter().filter(|part| part.length == length) {
            if part.start != had {
                break;
            }
            match self.kept(&part.url).await {
                Some(kept) if u64::from(kept.length()) == part.end - part.start + 1 => {
                    bytes.set(&kept, had as u32);
                    had = part.end + 1;
                }
                _ => break,
            }
        }
        (had > 0).then_some(Prefix { bytes, had })
    }

    /// Deletes `parts` from this version's cache.
    async fn forget(&self, parts: &[Part]) {
        let Some(cache) = &self.cache else {
            return;
        };
        for part in parts {
            // A part that stays is ignored once the file is kept whole, and goes with this version's cache.
            let _ = JsFuture::from(cache.delete_with_str(&part.url)).await;
        }
    }

    /// Deletes the caches of older versions of this model, once this version has every file.
    async fn forget_older(&self) {
        let Some(caches) = caches() else {
            return;
        };
        let Ok(names) = JsFuture::from(caches.keys()).await else {
            return;
        };
        let current = cache_name(self.name, self.version);
        let model = cache_name(self.name, "");
        for name in js_sys::Array::from(&names)
            .iter()
            .filter_map(|n| n.as_string())
        {
            if name.starts_with(&model) && name != current {
                let _ = JsFuture::from(caches.delete(&name)).await;
            }
        }
    }
}

/// This browser's Cache Storage; none where the page has none: `window.caches` exists in a secure context only (an
/// insecure page reads it as `undefined`), and a browser may withhold it.
fn caches() -> Option<CacheStorage> {
    web_sys::window()?
        .caches()
        .ok()
        .filter(|caches| !caches.is_undefined())
}

/// The Cache Storage cache of `name`'s browser files at `version`.
fn cache_name(name: &str, version: &str) -> String {
    format!("{CACHE_PREFIX} {name} {version}")
}

/// The URL `file` of `name`'s browser variant is kept under in Cache Storage: its path on a server, whichever source
/// sends it.
fn file_url(name: &str, file: &str) -> String {
    format!("/v1/browser/{name}/{file}")
}

/// The URL bytes `start` to `end` (both included) of `file`, of `length` bytes, are kept under while it downloads.
fn part_url(name: &str, file: &str, start: u64, end: u64, length: u64) -> String {
    format!("{}?bytes={start}-{end}/{length}", file_url(name, file))
}
