//! The llama.cpp runtime behind `ardana-core`'s runtime traits.
//!
//! A loaded model lives on its own worker thread, which owns the `LlamaModel` and the one `LlamaContext` borrowing it
//! (the context is `!Send`). [`LlamaModelHandle`] is the `Send` side: it sends one prompt per job over a channel and
//! waits for the label logits, so no `unsafe` is needed to satisfy `LoadedModel: Send`.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use anyhow::{Context, Result, anyhow, bail, ensure};
use ardana_core::runtime::{LoadOptions, LoadedModel, Runtime};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::token::LlamaToken;

/// decider's micro-batch: `n_ubatch = min(2048, n_ctx)` (`engine_gguf.py`).
const MAX_UBATCH: u32 = 2048;

/// The process-wide llama.cpp backend, initialised once with its log output silenced.
fn backend() -> Result<&'static LlamaBackend> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    static INIT: Mutex<()> = Mutex::new(());
    if let Some(backend) = BACKEND.get() {
        return Ok(backend);
    }
    let _guard = INIT
        .lock()
        .map_err(|_| anyhow!("llama.cpp backend init lock poisoned"))?;
    if let Some(backend) = BACKEND.get() {
        return Ok(backend);
    }
    // Silence llama.cpp and ggml (Metal device) logs before anything can emit one, so `ardana run` output stays JSON.
    llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default().with_logs_enabled(false));
    let backend = LlamaBackend::init().context("initialising the llama.cpp backend")?;
    Ok(BACKEND.get_or_init(|| backend))
}

/// llama.cpp for GGUF files, on Metal by default on macOS.
#[derive(Debug, Default, Clone, Copy)]
pub struct LlamaRuntime;

impl Runtime for LlamaRuntime {
    fn id(&self) -> &'static str {
        "llama.cpp"
    }

    /// A file starting with the GGUF magic.
    fn supports(&self, weights: &Path) -> bool {
        use std::io::Read;
        let mut magic = [0u8; 4];
        std::fs::File::open(weights)
            .and_then(|mut f| f.read_exact(&mut magic))
            .is_ok_and(|()| &magic == b"GGUF")
    }

    fn load(&self, weights: &Path, opts: &LoadOptions) -> Result<Box<dyn LoadedModel>> {
        Ok(Box::new(LlamaModelHandle::load(weights, opts)?))
    }
}

/// One prompt to decode on the worker thread.
struct Job {
    ids: Vec<u32>,
    slots: Vec<usize>,
    label_ids: Vec<u32>,
    reply: Sender<Result<Vec<Vec<f32>>>>,
}

/// A loaded GGUF: the channel to its worker thread.
pub struct LlamaModelHandle {
    jobs: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
    n_ctx: usize,
    path: PathBuf,
}

impl std::fmt::Debug for LlamaModelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaModelHandle")
            .field("path", &self.path)
            .field("n_ctx", &self.n_ctx)
            .finish()
    }
}

impl LlamaModelHandle {
    /// Starts the worker thread, which loads the model and creates its context; returns once both exist.
    pub fn load(weights: &Path, opts: &LoadOptions) -> Result<LlamaModelHandle> {
        let backend = backend()?;
        let gpu_layers = gpu_layers(opts.gpu_layers)?;
        let n_ctx = NonZeroU32::new(opts.n_ctx).context("n_ctx must be greater than 0")?;
        let path = weights.to_path_buf();
        let (ready_tx, ready_rx) = sync_channel::<Result<usize>>(1);
        let (jobs_tx, jobs_rx) = channel::<Job>();
        let worker_path = path.clone();
        let worker = std::thread::Builder::new()
            .name(format!(
                "llama {}",
                path.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            ))
            .spawn(move || {
                worker(
                    backend,
                    &worker_path,
                    gpu_layers,
                    n_ctx,
                    &ready_tx,
                    &jobs_rx,
                )
            })
            .context("spawning the llama.cpp worker thread")?;
        let loaded = ready_rx
            .recv()
            .map_err(|_| {
                anyhow!(
                    "the llama.cpp worker for {} exited while loading",
                    path.display()
                )
            })
            .and_then(|result| result);
        match loaded {
            Ok(n_ctx) => Ok(LlamaModelHandle {
                jobs: Some(jobs_tx),
                worker: Some(worker),
                n_ctx,
                path,
            }),
            Err(err) => {
                let _ = worker.join();
                Err(err)
            }
        }
    }
}

impl LoadedModel for LlamaModelHandle {
    fn n_ctx(&self) -> usize {
        self.n_ctx
    }

    fn slot_logits(
        &mut self,
        ids: &[u32],
        slots: &[usize],
        label_ids: &[u32],
    ) -> Result<Vec<Vec<f32>>> {
        let (reply, answer) = channel();
        let job = Job {
            ids: ids.to_vec(),
            slots: slots.to_vec(),
            label_ids: label_ids.to_vec(),
            reply,
        };
        let jobs = self.jobs.as_ref().context("the model is unloaded")?;
        jobs.send(job).map_err(|_| {
            anyhow!(
                "the llama.cpp worker for {} has stopped",
                self.path.display()
            )
        })?;
        answer.recv().map_err(|_| {
            anyhow!(
                "the llama.cpp worker for {} stopped while decoding",
                self.path.display()
            )
        })?
    }
}

impl Drop for LlamaModelHandle {
    /// Closes the job channel and waits for the worker to free the context and the model.
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// `LoadOptions::gpu_layers` as `with_n_gpu_layers` takes it: -1 offloads every layer (`u32::MAX`, which the crate
/// clamps to `i32::MAX` and llama.cpp reads as all), 0 keeps them on the CPU.
fn gpu_layers(gpu_layers: i32) -> Result<u32> {
    match gpu_layers {
        -1 => Ok(u32::MAX),
        n => u32::try_from(n).map_err(|_| anyhow!("gpu_layers must be -1 (all) or >= 0, got {n}")),
    }
}

/// The worker thread: owns the model and its context for the handle's lifetime and serves one job at a time.
fn worker(
    backend: &'static LlamaBackend,
    path: &Path,
    gpu_layers: u32,
    n_ctx: NonZeroU32,
    ready: &SyncSender<Result<usize>>,
    jobs: &Receiver<Job>,
) {
    // The params hold raw pointers (not `Send`), so they are built on this thread.
    let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
    let model = match LlamaModel::load_from_file(backend, path, &params) {
        Ok(model) => model,
        Err(err) => {
            let _ = ready.send(Err(anyhow!(
                "llama.cpp could not load {}: {err}",
                path.display()
            )));
            return;
        }
    };
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(n_ctx.get())
        .with_n_ubatch(n_ctx.get().min(MAX_UBATCH));
    let mut ctx = match model.new_context(backend, ctx_params) {
        Ok(ctx) => ctx,
        Err(err) => {
            let _ = ready.send(Err(anyhow!(
                "llama.cpp could not create a context for {}: {err}",
                path.display()
            )));
            return;
        }
    };
    let n_ctx = ctx.n_ctx() as usize;
    let n_vocab = usize::try_from(model.n_vocab()).unwrap_or(0);
    if ready.send(Ok(n_ctx)).is_err() {
        return;
    }
    for job in jobs {
        let result = decode(&mut ctx, n_ctx, n_vocab, &job);
        let _ = job.reply.send(result);
    }
}

/// One prompt alone in a cleared context, logits flagged only at the slots (decider `engine_gguf.py`).
fn decode(
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    n_ctx: usize,
    n_vocab: usize,
    job: &Job,
) -> Result<Vec<Vec<f32>>> {
    ensure!(!job.ids.is_empty(), "an empty prompt has no answer slot");
    ensure!(
        job.ids.len() <= n_ctx,
        "a prompt of {} tokens exceeds the context window of {n_ctx} tokens",
        job.ids.len()
    );
    if let Some(&bad) = job.label_ids.iter().find(|&&id| id as usize >= n_vocab) {
        bail!("label id {bad} is outside the model's {n_vocab}-token vocabulary");
    }
    if let Some(&bad) = job.slots.iter().find(|&&s| s >= job.ids.len()) {
        bail!("slot {bad} is outside the {}-token prompt", job.ids.len());
    }
    ctx.clear_kv_cache();
    let mut batch = LlamaBatch::new(job.ids.len(), 1);
    for (pos, &id) in job.ids.iter().enumerate() {
        let token =
            LlamaToken::new(i32::try_from(id).context("token id does not fit llama.cpp's i32")?);
        let pos_i32 = i32::try_from(pos).context("position does not fit llama.cpp's i32")?;
        batch.add(token, pos_i32, &[0], job.slots.contains(&pos))?;
    }
    ctx.decode(&mut batch).context("llama_decode")?;
    job.slots
        .iter()
        .map(|&slot| {
            let row = ctx.get_logits_ith(i32::try_from(slot)?);
            Ok(job.label_ids.iter().map(|&id| row[id as usize]).collect())
        })
        .collect()
}
