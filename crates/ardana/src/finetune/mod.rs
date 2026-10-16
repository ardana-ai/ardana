//! `ardana finetune`: a LoRA fine-tune of any model `ardana` runs into a System One model (Q1, Q6, Q7), from the
//! transformers checkpoint the base names or `--checkpoint` does ([`ardana_registry::base`]). The data, JSONL or CSV,
//! is read and checked against the base's own tokenizer and profile before anything heavy happens, every labelled
//! question becoming the rows `Decider::plan` builds ([`ardana_core::finetune`]); then the environment in
//! `$ARDANA_HOME/finetune/` is built or reused ([`env`]), the base's checkpoint downloads to the Hugging Face cache,
//! TRL's `sft` script trains the adapter on those rows, `merge.py` merges it, and llama.cpp's converter writes the GGUF
//! and the linked llama.cpp quantizes it, or, under `--format safetensors`, the merged checkpoint is the model, which
//! Candle runs (Q9, Q10). Its temperatures are then fitted on the calibration split through that model as `ardana run`
//! loads it, and the base and the model answer the test split for the report ([`evaluate`]). The model is built in
//! the run's working directory and only moved to `$ARDANA_HOME/models/<name>/` and registered once whole, so a run
//! that stops (SIGINT, a failing trainer) leaves neither. Before the checkpoint downloads, torch in the environment
//! says which devices it finds, and the run is held against the memory of the one it trains on ([`device`]).

mod device;
mod env;
mod evaluate;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use ardana_core::calibrate::Fitted;
use ardana_core::finetune::{self, Example, TrainingRow};
use ardana_core::{Decider, Layout, snapshot};
use ardana_registry::base::{Base, BaseModel, BaseOptions, Checkpoint};
use ardana_registry::library::{Library, LibraryClient, library_name};
use ardana_registry::refs::{DEFAULT_QUANT, matches_quant};
use ardana_registry::{LayoutKind, Named, PullOptions, Registry, ResolvedModel, TOKENIZER_EXTRAS};
use clap::{Args, ValueEnum};
use serde_json::{Value, json};
use tokio::process::Command;

pub use device::Device;
pub use env::{LLAMA_CPP_COMMIT, PYTHON, UV_VERSION};

/// The Q23 settings: LoRA rank and alpha over every linear layer, the learning rate and the seed; the epochs are
/// `--epochs`.
pub const LORA_RANK: u32 = 16;
pub const LORA_ALPHA: u32 = 32;
pub const LEARNING_RATE: &str = "1e-4";
pub const DEFAULT_EPOCHS: u32 = 2;
pub const SEED: u64 = finetune::SEED;
/// Where models `ardana finetune` made live in the Ardana home, one directory each.
pub const MODELS_DIR: &str = "models";
/// The file whose presence in a model's directory marks a model `ardana finetune` made.
pub const RECORD: &str = "finetune.json";
/// The message of a run SIGINT stopped.
const INTERRUPTED: &str = "interrupted; nothing was registered and the run's files were removed";

#[derive(Debug, Args)]
pub struct FinetuneArgs {
    /// The model to fine-tune: a library size <family>:<size>, a pulled model's name, hf.co/<org>/<repo>[:<quant>],
    /// hf.co/<org>/<repo>:<file>.gguf, ollama:[<namespace>/]<name>[:<tag>] or a local GGUF file
    #[arg(value_name = "BASE")]
    pub base: String,
    /// The labelled data: a JSONL file, one state with its answers a line, or a CSV file (.csv) with a state column
    /// and a column per question id of --questions
    #[arg(value_name = "DATA")]
    pub data: PathBuf,
    /// The name to register the model under, which no pulled model or library model may hold
    #[arg(long)]
    pub name: String,
    /// The questions of the lines that carry none: a map of question id to spec, or a /v1/systemone body
    #[arg(long, value_name = "FILE")]
    pub questions: Option<PathBuf>,
    /// Passes over the training lines; the one with the lowest calibration loss is kept
    #[arg(long, default_value_t = DEFAULT_EPOCHS, value_parser = clap::value_parser!(u32).range(1..))]
    pub epochs: u32,
    /// What the model is written as: a GGUF llama.cpp runs, or a safetensors checkpoint Candle runs, for the causal
    /// architectures Candle implements
    #[arg(long, value_enum, default_value_t = Format::Gguf)]
    pub format: Format,
    /// The GGUF's quant, a llama.cpp file type (q8_0, q4_k_m, ...); default the base's own GGUF quant
    #[arg(long)]
    pub quant: Option<String>,
    /// The transformers checkpoint to train from, hf.co/<org>/<repo> or a directory; default the one the base names
    #[arg(long, value_name = "CHECKPOINT")]
    pub checkpoint: Option<String>,
    /// Let a checkpoint whose config.json has auto_map run the Python code it ships, as transformers' option of the
    /// same name does
    #[arg(long)]
    pub trust_remote_code: bool,
    /// Where to train: auto takes cuda, then mps, then cpu, as torch finds them; bfloat16 on cuda and mps, float32 on
    /// cpu
    #[arg(long, value_enum, default_value_t = Device::Auto)]
    pub device: Device,
}

/// What `--format` writes (Q9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// `<name>-<QUANT>.gguf`, quantized by the linked llama.cpp.
    Gguf,
    /// The merged checkpoint at the training dtype: `config.json` and its safetensors.
    Safetensors,
}

impl Format {
    /// The name `--format` and `finetune.json#format` give it.
    fn name(self) -> &'static str {
        match self {
            Format::Gguf => "gguf",
            Format::Safetensors => "safetensors",
        }
    }
}

/// `ardana finetune`, on Linux and macOS (Q21).
pub fn finetune(args: &FinetuneArgs) -> Result<()> {
    if !matches!(std::env::consts::OS, "linux" | "macos") {
        bail!(
            "fine-tuning runs on Linux and macOS; this is {}",
            std::env::consts::OS
        );
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(run(args))
}

async fn run(args: &FinetuneArgs) -> Result<()> {
    let started = Instant::now();
    let mut interrupt = Interrupt::listen()?;
    // Absolute: the Python steps run in the run's working directory.
    let home = std::path::absolute(Registry::home()?)?;
    let client = LibraryClient::from_env()?;
    let name = args.name.as_str();
    let models = home.join(MODELS_DIR).join(name);
    let registry = Registry::open(&home)?;
    free_name(name, &registry, &client, &models).await?;
    match (&args.quant, args.format) {
        (Some(_), Format::Safetensors) => bail!(
            "--quant names a GGUF's quant, and --format safetensors writes no GGUF: the checkpoint keeps the \
             training dtype, float32 on the CPU and bfloat16 on CUDA and MPS; drop --quant, or pass --format gguf"
        ),
        (Some(quant), Format::Gguf) => quant_is_written(quant, &format!("--quant {quant}"))?,
        (None, _) => {}
    }

    // The base's checkpoint, tokenizer and profile, then the data, before anything heavy (R1.4).
    let library = client.lookup(&args.base).await?;
    let opts = BaseOptions {
        checkpoint: args.checkpoint.as_deref(),
        trust_remote_code: args.trust_remote_code,
        progress: true,
    };
    let base = interrupt
        .or(async { Ok(ardana_registry::base::base(&args.base, &library, &registry, opts).await?) })
        .await?;
    eprintln!(
        "fine-tuning {} from the checkpoint {}",
        base.name, base.checkpoint
    );
    let quant = match (&args.quant, args.format) {
        (_, Format::Safetensors) => {
            candle_writes(&base.checkpoint.to_string(), &base.architectures)?;
            None
        }
        (Some(quant), Format::Gguf) => Some(quant.clone()),
        (None, Format::Gguf) => {
            let quant = base_quant(&base);
            quant_is_written(&quant, &format!("the base's own quant {quant}"))?;
            Some(quant)
        }
    };
    let decider = Decider::new(
        ardana_registry::load_tokenizer(&base.tokenizer)?,
        base.profile.clone(),
    )?;
    let data = read_data(args, &decider)?;
    let split = finetune::split(&data.examples);
    for sparse in finetune::sparse_options(&split.train) {
        eprintln!(
            "warning: question {} has {} training examples of {}, fewer than {}",
            sparse.question,
            sparse.examples,
            sparse.option,
            finetune::MIN_OPTION_EXAMPLES
        );
    }
    let train = rows(&decider, &split.train)?;
    let calibration = rows(&decider, &split.calibration)?;
    let longest = train
        .iter()
        .chain(&calibration)
        .map(|row| row.input_ids.len())
        .max()
        .unwrap_or(0);
    eprintln!(
        "{}: {} labelled lines ({} without labels skipped): {} to train on ({} rows), {} to calibrate on ({} rows), {} \
         to test on; the longest row {longest} tokens",
        args.data.display(),
        data.examples.len(),
        data.unlabelled,
        split.train.len(),
        train.len(),
        split.calibration.len(),
        calibration.len(),
        split.test.len()
    );

    let env = env::prepare(&home, &mut interrupt).await?;
    let work = Work::create(env.work(name))?;
    // The device and the memory it has, before the checkpoint downloads (Q4, Q16).
    let found = device::probe(&env, &work, &mut interrupt).await?;
    let device = device::pick(args.device, &found)?;
    let bytes = interrupt
        .or(async { Ok(base.weight_bytes().await?) })
        .await?;
    let shape = device::Shape::of(bytes, &base.config)?;
    let training = device::fit(device, &found, shape, longest as u64, &base.name)?;
    match &base.checkpoint {
        Checkpoint::Hub(_) => eprintln!("downloading the checkpoint {}", base.checkpoint),
        Checkpoint::Dir { .. } => eprintln!("reading the checkpoint {}", base.checkpoint),
    }
    let checkpoint = interrupt
        .or(async { Ok(base.download(true).await?) })
        .await?;
    // The base the report reads, as `ardana run` reads it: a library size in its default quant and profile, pulled when
    // missing; an entry as recorded; a reference as `ardana pull` resolves it (downloaded, not recorded).
    let pulled = match &base.model {
        BaseModel::Library(_) => match Registry::open(&home)?.named(&base.name, &library)? {
            Named::Pulled(model) => model.clone(),
            Named::Library(pick) => interrupt.or(crate::pull_first(&pick)).await?,
        },
        BaseModel::Pulled(model) => (**model).clone(),
        BaseModel::Reference { reference, opts } => {
            eprintln!("reading the base {reference} as `ardana pull` reads it");
            let runtimes = crate::runtimes();
            interrupt
                .or(async {
                    Ok(
                        ardana_registry::pull(reference, opts, &runtimes, &Library::empty())
                            .await?,
                    )
                })
                .await?
        }
    };

    let trained = text_tower(&checkpoint, &work.dir)?;
    let out = work.dir.join("out");
    std::fs::create_dir_all(&out)?;
    write_rows(&work.dir.join("data/train.jsonl"), &train)?;
    write_rows(&work.dir.join("data/validation.jsonl"), &calibration)?;

    let dtype = training.dtype;
    eprintln!(
        "training {name} from {} on {} in {dtype}{}: {} rows, {} epoch{}",
        base.name,
        training.device.name(),
        if training.qlora { " with QLoRA" } else { "" },
        train.len(),
        args.epochs,
        if args.epochs == 1 { "" } else { "s" }
    );
    let adapter = work.dir.join("adapter");
    let mut trainer = python(&env, &work);
    trainer
        .args(["-I", "-m", "trl.scripts.sft", "--model_name_or_path"])
        .arg(&trained)
        .arg("--dataset_name")
        .arg(work.dir.join("data"))
        .args(["--dataset_train_split", "train"])
        .args(["--dataset_test_split", "validation"])
        .arg("--output_dir")
        .arg(&adapter)
        .args(["--use_peft", "True", "--lora_target_modules", "all-linear"])
        .args(["--lora_r", &LORA_RANK.to_string()])
        .args(["--lora_alpha", &LORA_ALPHA.to_string()])
        .args(["--learning_rate", LEARNING_RATE])
        .args(["--num_train_epochs", &args.epochs.to_string()])
        .args(["--seed", &SEED.to_string()])
        // The loss reads the answer token alone, and no row is cut.
        .args(["--completion_only_loss", "True"])
        .args(["--max_length", &longest.to_string()])
        // The epoch with the lowest calibration loss is the adapter saved.
        .args(["--eval_strategy", "epoch", "--save_strategy", "epoch"])
        .args(["--load_best_model_at_end", "True"])
        .args([
            "--metric_for_best_model",
            "eval_loss",
            "--greater_is_better",
            "False",
        ])
        .args(["--dtype", dtype])
        .args(["--report_to", "none"]);
    if training.device == Device::Cpu {
        // On the CPU, memory is the system's; recomputing activations would cost a quarter of the time.
        trainer.args([
            "--bf16",
            "False",
            "--use_cpu",
            "True",
            "--gradient_checkpointing",
            "False",
        ]);
    } else {
        trainer.args(["--bf16", "True", "--gradient_checkpointing", "True"]);
    }
    if training.qlora {
        // Q16: the base's linear layers in 4-bit NF4 (bitsandbytes), computing in bfloat16, the adapter over them.
        trainer.args(["--load_in_4bit", "True", "--bnb_4bit_quant_type", "nf4"]);
    }
    if args.trust_remote_code {
        trainer.args(["--trust_remote_code", "True"]);
    }
    env::run(&mut trainer, &mut interrupt, "the trainer (TRL sft)").await?;

    let merged = work.dir.join("merged");
    let script = work.dir.join("merge.py");
    std::fs::write(&script, include_str!("merge.py"))?;
    let mut merge = python(&env, &work);
    merge
        .arg("-I")
        .arg(&script)
        .arg(&trained)
        .arg(&adapter)
        .arg(&merged)
        .arg(dtype);
    if args.trust_remote_code {
        merge.arg("--trust-remote-code");
    }
    env::run(&mut merge, &mut interrupt, "merging the adapter (merge.py)").await?;

    // The GGUF's file in the model's directory; a checkpoint is the directory itself.
    let file = match &quant {
        Some(quant) => {
            copy_tokenizer_files(&trained, &merged)?;
            let converted = work.dir.join(format!("{name}-float32.gguf"));
            let mut convert = python(&env, &work);
            // Not isolated: the converter imports its `conversion/` package from beside itself.
            convert
                .args(["-E", "-s"])
                .arg(env.converter())
                .arg(&merged)
                .args(["--outtype", "f32", "--model-name", name, "--outfile"])
                .arg(&converted);
            if declares_mtp(&checkpoint)? {
                // A multi-token-prediction head the checkpoint declares without weights; Ardana never drafts
                // (onnx.md).
                convert.arg("--no-nextn");
            }
            env::run(&mut convert, &mut interrupt, "llama.cpp's converter").await?;

            let file = format!("{name}-{}.gguf", quant.to_ascii_uppercase());
            let gguf = out.join(&file);
            eprintln!("quantizing {name} to {quant}");
            // llama.cpp cannot be stopped mid-file: SIGINT ends the run once it is done.
            interrupt
                .blocking({
                    let quant = quant.clone();
                    move |_| ardana_llama::quantize(&converted, &gguf, &quant)
                })
                .await
                .context("quantizing")?;
            Some(file)
        }
        None => {
            write_checkpoint(&trained, &merged, &out, dtype)?;
            None
        }
    };
    let weights = |dir: &Path| match &file {
        Some(file) => dir.join(file),
        None => dir.to_path_buf(),
    };

    // Q11, Q30: the temperatures are fitted through the model itself, read as `ardana run` reads the entry it becomes.
    write_model(&out, &base, &split, args, training, quant.as_deref())?;
    let model = resolve(&weights(&out), name, base.layout).await?;
    let calibration = owned(&split.calibration);
    eprintln!("calibrating {name} on {} lines", split.calibration.len());
    let (fitted, runtime) = interrupt
        .blocking(move |stop| evaluate::calibrate(&model, &calibration, stop))
        .await?;
    eprintln!("calibrated {name} through {runtime}");
    eprintln!("{name} answers at {}", temperatures(&fitted));
    set_temperatures(&out, &fitted)?;
    let model = resolve(&weights(&out), name, base.layout).await?;

    eprintln!(
        "answering the {} test lines with {} and {name}",
        split.test.len(),
        pulled.name
    );
    let test = owned(&split.test);
    let (base_graded, graded) = interrupt
        .blocking(move |stop| {
            Ok((
                evaluate::answer(&pulled, &test, stop)?,
                evaluate::answer(&model, &test, stop)?,
            ))
        })
        .await?;
    let report = evaluate::Report::new(
        split.test.len(),
        (&base.name, &base_graded),
        (name, &graded),
    );
    let record = out.join(RECORD);
    let mut written: Value = serde_json::from_str(&std::fs::read_to_string(&record)?)?;
    written["report"] = report.to_json();
    std::fs::write(&record, serde_json::to_string_pretty(&written)? + "\n")?;

    std::fs::create_dir_all(home.join(MODELS_DIR))?;
    std::fs::rename(&out, &models)
        .with_context(|| format!("moving the model to {}", models.display()))?;
    let model = match register(&weights(&models), name, base.layout).await {
        Ok(model) => model,
        Err(err) => {
            std::fs::remove_dir_all(&models).ok();
            return Err(err);
        }
    };
    drop(work);
    print!("{}", report.table());
    let what = match &quant {
        Some(quant) => quant.clone(),
        None => format!("{} in {dtype}", args.format.name()),
    };
    println!(
        "fine-tuned {name} from {} ({what}, {}) in {} in {} min; try `ardana run {name}`",
        base.name,
        crate::size(&model),
        models.display(),
        started.elapsed().as_secs() / 60
    );
    Ok(())
}

/// The examples of a split, owned, for a thread of their own.
fn owned(examples: &[&Example]) -> Vec<Example> {
    examples.iter().map(|&example| example.clone()).collect()
}

/// The checkpoint as the trainer and `merge.py` load it. A multimodal checkpoint (its `config.json` names a
/// `*ForConditionalGeneration` and holds a `text_config`) trains its text tower alone, the model Ardana runs: `<work>/
/// text/` holds the text tower's `config.json` (the `text_config`, its architecture the `*ForCausalLM` of the same
/// name) and links to the checkpoint's files but its processor's, so transformers loads the text model from the
/// checkpoint's `language_model` tensors and TRL reads the tokenizer, not a multimodal processor. Any other checkpoint
/// is read as it is.
fn text_tower(checkpoint: &Path, work: &Path) -> Result<PathBuf> {
    const PROCESSOR_FILES: [&str; 3] = [
        "processor_config.json",
        "preprocessor_config.json",
        "video_preprocessor_config.json",
    ];
    let path = checkpoint.join(snapshot::CONFIG);
    let config: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    let multimodal = config["architectures"][0]
        .as_str()
        .and_then(|arch| arch.strip_suffix("ForConditionalGeneration"));
    let (Some(prefix), Value::Object(text)) = (multimodal, &config["text_config"]) else {
        return Ok(checkpoint.to_path_buf());
    };
    let mut text = text.clone();
    text.insert(
        "architectures".into(),
        json!([format!("{prefix}ForCausalLM")]),
    );
    let dir = work.join("text");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join(snapshot::CONFIG),
        serde_json::to_string_pretty(&text)?,
    )?;
    for entry in std::fs::read_dir(checkpoint)? {
        let path = entry?.path();
        let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        if file != snapshot::CONFIG && !PROCESSOR_FILES.contains(&file) {
            link(&std::fs::canonicalize(&path)?, &dir.join(file))?;
        }
    }
    Ok(dir)
}

#[cfg(unix)]
fn link(target: &Path, path: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, path)
        .with_context(|| format!("linking {} to {}", path.display(), target.display()))
}

#[cfg(not(unix))]
fn link(_: &Path, _: &Path) -> Result<()> {
    bail!("fine-tuning runs on Linux and macOS")
}

/// Refuses a name that is no single directory name, and one that is taken (Q27): an entry's, a library family's or
/// a library name's (any spelling of them), or one whose model directory exists; nothing is ever overwritten.
async fn free_name(
    name: &str,
    registry: &Registry,
    client: &LibraryClient,
    dir: &Path,
) -> Result<()> {
    let valid = !name.is_empty()
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\'));
    ensure!(
        valid,
        "the name {name:?} is no name: a name is a word without whitespace, `/` or `\\` that does not start with `.`"
    );
    let pick = "pick another --name";
    if registry.entry(name).is_ok() {
        bail!("the name {name} is taken by a pulled model; {pick}, or `ardana rm {name}` first");
    }
    let library = client.lookup(name).await?;
    if let Some(family) = library_name(name)
        && let Some(taken) = library
            .models
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(family))
    {
        let what = match library.find(name) {
            Ok(Some(pick)) if name.contains(':') => format!("the library model {}", pick.name()),
            _ => format!("the library family {}", taken.name),
        };
        bail!("the name {name} is taken by {what}; {pick}");
    }
    ensure!(
        !dir.exists(),
        "the name {name} is taken by the directory {}; {pick}, or remove it",
        dir.display()
    );
    Ok(())
}

/// The quant a run writes without `--quant` (Q24): the quant of the base's own GGUF, which the library or the
/// reference names or the GGUF's file name ends in (`-Q8_0.gguf`), else q4_k_m.
fn base_quant(base: &Base) -> String {
    base.quant
        .clone()
        .or_else(|| {
            let file = base.gguf.as_deref()?;
            ardana_llama::QUANTS
                .iter()
                .find(|(quant, _)| matches_quant(file, quant))
                .map(|(quant, _)| quant.to_string())
        })
        .unwrap_or_else(|| DEFAULT_QUANT.to_ascii_lowercase())
}

/// Refuses a quant, `what` naming where it comes from, that the linked llama.cpp does not write.
fn quant_is_written(quant: &str, what: &str) -> Result<()> {
    ensure!(
        ardana_llama::is_quant(quant),
        "{what} is no llama.cpp file type ardana writes; --quant takes {}",
        ardana_llama::QUANTS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

/// The data file (CSV by its `.csv` extension, else JSONL) and the `--questions` file, read and checked.
fn read_data(args: &FinetuneArgs, decider: &Decider) -> Result<finetune::Data> {
    let questions = match &args.questions {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            Some(finetune::read_questions(
                &text,
                &path.display().to_string(),
            )?)
        }
        None => None,
    };
    let text = std::fs::read_to_string(&args.data)
        .with_context(|| format!("reading {}", args.data.display()))?;
    let file = args.data.display().to_string();
    let csv = args
        .data
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"));
    Ok(if csv {
        finetune::read_csv(&text, &file, questions.as_ref(), decider)?
    } else {
        finetune::read_jsonl(&text, &file, questions.as_ref(), decider)?
    })
}

/// The training rows of `examples`, in line order.
fn rows(decider: &Decider, examples: &[&Example]) -> Result<Vec<TrainingRow>> {
    let mut rows = Vec::new();
    for example in examples {
        rows.extend(
            finetune::training_rows(decider, example)
                .with_context(|| format!("planning line {}", example.line))?,
        );
    }
    Ok(rows)
}

/// `rows` as the dataset TRL reads: one `{"input_ids", "completion_mask"}` object a line.
fn write_rows(path: &Path, rows: &[TrainingRow]) -> Result<()> {
    let mut text = String::new();
    for row in rows {
        text.push_str(
            &json!({"input_ids": row.input_ids, "completion_mask": row.completion_mask})
                .to_string(),
        );
        text.push('\n');
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The venv's interpreter, offline on the hub cache (Q29), with its Hugging Face home and temporary files in the
/// run's working directory; it writes no bytecode (`-B`), so the environment's files stay as they were built.
fn python(env: &env::Env, work: &Work) -> Command {
    let mut cmd = Command::new(env.python());
    cmd.arg("-B")
        .current_dir(&work.dir)
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("HF_HOME", work.dir.join("hf"))
        .env("TMPDIR", &work.tmp)
        .env("PYTHONUNBUFFERED", "1");
    cmd
}

/// Copies the checkpoint's files that `save_pretrained` does not write (the tokenizer's) into the merged model, which
/// the converter reads them from.
fn copy_tokenizer_files(checkpoint: &Path, merged: &Path) -> Result<()> {
    for entry in std::fs::read_dir(checkpoint)? {
        let path = entry?.path();
        let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        let written = merged.join(file);
        if path.is_file()
            && !written.exists()
            && !file.ends_with(".safetensors")
            && !file.ends_with(".index.json")
        {
            std::fs::copy(&path, &written)
                .with_context(|| format!("copying {}", path.display()))?;
        }
    }
    Ok(())
}

/// The architectures `--format safetensors` writes (Q10): those of `ardana_candle::ARCHITECTURES` but THX-01's
/// decision model, the causal LMs Candle runs, so the set grows with Candle's.
fn safetensors_architectures() -> impl Iterator<Item = &'static str> {
    ardana_candle::ARCHITECTURES
        .into_iter()
        .filter(|&architecture| architecture != ardana_candle::thx01::ARCHITECTURE)
}

/// Refuses `--format safetensors` for the checkpoint `what` unless one of its `architectures` is Candle's
/// ([`safetensors_architectures`]), as Candle picks the first name it implements: before any weight downloads, naming
/// the architectures and `--format gguf`.
fn candle_writes(what: &str, architectures: &[String]) -> Result<()> {
    if architectures
        .iter()
        .any(|named| safetensors_architectures().any(|candle| candle == named))
    {
        return Ok(());
    }
    let named = if architectures.is_empty() {
        format!("its {} names no architecture", snapshot::CONFIG)
    } else {
        format!("its architecture {}", architectures.join(", "))
    };
    bail!(
        "{what}: {named} is no causal LM Candle runs ({}), so --format safetensors cannot write a model ardana runs; \
         pass --format gguf",
        safetensors_architectures().collect::<Vec<_>>().join(", ")
    )
}

/// The merged checkpoint as the model (`--format safetensors`): the safetensors `merge.py` saved (with their index
/// when sharded) moved into `out`, beside the `config.json` of the checkpoint as trained, its dtype `dtype`, the one
/// merged in. Merging changes no setting of the model, and `save_pretrained`'s own `config.json` is transformers 5's
/// (`rope_parameters` for `rope_theta`), which candle-transformers' configurations do not read.
fn write_checkpoint(trained: &Path, merged: &Path, out: &Path, dtype: &str) -> Result<()> {
    let mut files = snapshot::weight_files(merged).map_err(anyhow::Error::msg)?;
    let index = merged.join(snapshot::INDEX);
    if index.is_file() {
        files.push(index);
    }
    for path in files {
        let file = path.file_name().context("a weight file's name")?;
        std::fs::rename(&path, out.join(file))
            .with_context(|| format!("moving {}", path.display()))?;
    }
    let path = trained.join(snapshot::CONFIG);
    let mut config: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    for key in ["dtype", "torch_dtype"] {
        if config.get(key).is_some() {
            config[key] = json!(dtype);
        }
    }
    std::fs::write(
        out.join(snapshot::CONFIG),
        serde_json::to_string_pretty(&config)? + "\n",
    )?;
    Ok(())
}

/// Whether the checkpoint's `config.json` (or its text model's) declares a multi-token-prediction head.
fn declares_mtp(checkpoint: &Path) -> Result<bool> {
    let path = checkpoint.join(snapshot::CONFIG);
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    let declares = |config: &serde_json::Value| {
        ["mtp_num_hidden_layers", "num_nextn_predict_layers"]
            .iter()
            .any(|key| config[key].as_u64().is_some_and(|n| n > 0))
    };
    Ok(declares(&config) || declares(&config["text_config"]))
}

/// The model's files besides its weights: the base's tokenizer with its chat template files, `decider_config.json` in
/// the base's layout and score-level mode (Q7) and `finetune.json`, the record of the run (`quant` null for a
/// checkpoint; the device, dtype and QLoRA it trained with).
fn write_model(
    out: &Path,
    base: &Base,
    split: &finetune::Split<'_>,
    args: &FinetuneArgs,
    training: device::Training,
    quant: Option<&str>,
) -> Result<()> {
    let dir = base
        .tokenizer
        .parent()
        .context("the tokenizer's directory")?;
    for file in std::iter::once(ardana_registry::TOKENIZER_FILE).chain(TOKENIZER_EXTRAS) {
        let path = dir.join(file);
        if path.is_file() {
            std::fs::copy(&path, out.join(file))
                .with_context(|| format!("copying {}", path.display()))?;
        }
    }
    let layout = match base.profile.layout {
        Layout::Plain => "plain",
        Layout::Chat { .. } => "chat",
    };
    let config = json!({
        "version": args.name,
        "layout": layout,
        "isolated_levels": base.profile.isolated_levels,
        "temperature": 1.0,
        "release_date": ardana_registry::today(),
    });
    let record = json!({
        "base": base.name,
        "checkpoint": base.checkpoint,
        "format": args.format.name(),
        "quant": quant,
        "device": training.device.name(),
        "dtype": training.dtype,
        "qlora": training.qlora,
        "epochs": args.epochs,
        "rows": {
            "train": split.train.len(),
            "calibration": split.calibration.len(),
            "test": split.test.len(),
        },
        "license": base.license,
        "report": null,
    });
    for (file, value) in [(ardana_registry::DECIDER_CONFIG, config), (RECORD, record)] {
        std::fs::write(out.join(file), serde_json::to_string_pretty(&value)? + "\n")?;
    }
    Ok(())
}

/// The entry of the model's `weights`, not recorded: for a GGUF the one `ardana pull <gguf> --name <name> --layout
/// <layout>` would record, the tokenizer and `decider_config.json` beside it giving the profile; for a checkpoint
/// directory the same from the files in it, its runtime the first that supports it, as `ardana pull` picks one.
async fn resolve(weights: &Path, name: &str, layout: LayoutKind) -> Result<ResolvedModel> {
    if weights.is_dir() {
        let tokenizer = weights.join(ardana_registry::TOKENIZER_FILE);
        let profile = ardana_registry::read_profile(
            &tokenizer,
            &ardana_registry::load_tokenizer(&tokenizer)?,
            Some(&weights.join(ardana_registry::DECIDER_CONFIG)),
            layout,
            name,
        )?;
        let runtime = crate::runtimes()
            .for_weights(weights)
            .with_context(|| format!("no runtime of this ardana loads {}", weights.display()))?
            .id()
            .to_string();
        return Ok(ResolvedModel {
            name: name.to_string(),
            source: weights.display().to_string(),
            weights: weights.to_path_buf(),
            tokenizer,
            runtime,
            profile,
            pulled_at: ardana_registry::today(),
        });
    }
    let reference = weights
        .to_str()
        .with_context(|| format!("{} is no UTF-8 path", weights.display()))?;
    let opts = PullOptions {
        name: Some(name.to_string()),
        layout: Some(layout),
        ..PullOptions::default()
    };
    Ok(ardana_registry::pull(
        reference,
        &opts,
        &crate::runtimes(),
        &ardana_registry::library::Library::empty(),
    )
    .await?)
}

/// Records the model's `weights` in the registry as [`resolve`] reads them.
async fn register(weights: &Path, name: &str, layout: LayoutKind) -> Result<ResolvedModel> {
    let (model, _) = crate::record(resolve(weights, name, layout).await?)?;
    Ok(model)
}

/// Writes the fitted temperatures into the model's `decider_config.json`: `temperature`, and `temperature_by_type`
/// for the types that have their own (Q30).
fn set_temperatures(out: &Path, fitted: &Fitted) -> Result<()> {
    let path = out.join(ardana_registry::DECIDER_CONFIG);
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    config["temperature"] = json!(fitted.temperature);
    config["temperature_by_type"] = json!(fitted.temperature_by_type);
    std::fs::write(&path, serde_json::to_string_pretty(&config)? + "\n")?;
    Ok(())
}

/// The fitted temperatures as the run reports them: `temperature T over N answers`, then each type's own.
fn temperatures(fitted: &Fitted) -> String {
    let answers: usize = fitted.answers.values().sum();
    let mut text = format!(
        "temperature {} over {answers} calibration answers",
        fitted.temperature
    );
    for (kind, n) in &fitted.answers {
        match fitted.temperature_by_type.get(kind) {
            Some(t) => text.push_str(&format!("; {kind} {t} over {n}")),
            None => text.push_str(&format!(
                "; {kind} takes it ({n}, fewer than {})",
                ardana_core::calibrate::MIN_ANSWERS
            )),
        }
    }
    text
}

/// A run's working directory, removed with everything in it when the run ends, whatever its end.
struct Work {
    dir: PathBuf,
    tmp: PathBuf,
}

impl Work {
    fn create(dir: PathBuf) -> Result<Work> {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("emptying {}", dir.display()))?;
        }
        let tmp = dir.join("tmp");
        std::fs::create_dir_all(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        Ok(Work { dir, tmp })
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// SIGINT during a run: listened for from the start, so the signal stops the run (its children with it) instead of
/// the process, and the run removes what it built.
struct Interrupt {
    #[cfg(unix)]
    signal: tokio::signal::unix::Signal,
}

impl Interrupt {
    fn listen() -> Result<Interrupt> {
        Ok(Interrupt {
            #[cfg(unix)]
            signal: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .context("listening for SIGINT")?,
        })
    }

    /// Resolves when SIGINT arrives.
    async fn recv(&mut self) {
        #[cfg(unix)]
        self.signal.recv().await;
        #[cfg(not(unix))]
        std::future::pending::<()>().await;
    }

    /// `work`, unless SIGINT arrives first.
    async fn or<T>(&mut self, work: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::select! {
            done = work => done,
            () = self.recv() => bail!(INTERRUPTED),
        }
    }

    /// `work` on a blocking thread. SIGINT raises the flag it is given, which it may check to stop early; the run then
    /// ends once `work` returns.
    async fn blocking<T: Send + 'static>(
        &mut self,
        work: impl FnOnce(&AtomicBool) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let stop = Arc::new(AtomicBool::new(false));
        let task = tokio::task::spawn_blocking({
            let stop = stop.clone();
            move || work(&stop)
        });
        tokio::pin!(task);
        tokio::select! {
            done = &mut task => done.context("a blocking step panicked")?,
            () = self.recv() => {
                stop.store(true, Ordering::Relaxed);
                let _ = task.await;
                bail!(INTERRUPTED);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R5.1: CUDA's wheels, by uv's driver-aware `--torch-backend auto`, on Linux where `nvidia-smi` answers, and only
    /// there bitsandbytes; PyPI's wheels (no `--torch-backend`) on macOS arm64; PyTorch's CPU wheels on the rest.
    #[test]
    fn torch_build_follows_the_machine() {
        use env::{BITSANDBYTES, PACKAGES, TorchBuild};
        for (os, arch, nvidia, build, backend) in [
            ("linux", "x86_64", true, TorchBuild::Cuda, Some("auto")),
            ("linux", "aarch64", true, TorchBuild::Cuda, Some("auto")),
            ("linux", "x86_64", false, TorchBuild::Cpu, Some("cpu")),
            ("linux", "aarch64", false, TorchBuild::Cpu, Some("cpu")),
            ("macos", "aarch64", false, TorchBuild::Pypi, None),
            ("macos", "x86_64", false, TorchBuild::Cpu, Some("cpu")),
        ] {
            let machine = format!("{arch} {os}, nvidia-smi answering: {nvidia}");
            let picked = TorchBuild::of(os, arch, nvidia);
            assert_eq!(picked, build, "{machine}");
            assert_eq!(picked.backend(), backend, "{machine}");
            let packages = picked.packages();
            assert_eq!(packages[..PACKAGES.len()], PACKAGES, "{machine}");
            let extra = &packages[PACKAGES.len()..];
            if build == TorchBuild::Cuda {
                assert_eq!(extra, [BITSANDBYTES], "{machine}");
            } else {
                assert!(extra.is_empty(), "{machine}: {extra:?}");
            }
        }
    }

    /// R4.3: `--format safetensors` accepts every name of `ardana_candle::ARCHITECTURES` but THX-01's decision model,
    /// read from the list itself, also as one of several names; anything else is refused naming it and `--format gguf`.
    #[test]
    fn safetensors_follows_candle_architectures() {
        let names = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        for architecture in ardana_candle::ARCHITECTURES {
            let written = candle_writes("a checkpoint", &names(&[architecture]));
            if architecture == ardana_candle::thx01::ARCHITECTURE {
                let err = written.expect_err("THX-01 accepted").to_string();
                assert!(
                    err.contains(&format!("its architecture {architecture} is no causal LM")),
                    "{err}"
                );
            } else {
                written.unwrap_or_else(|err| panic!("{architecture} refused: {err}"));
                candle_writes("a checkpoint", &names(&["Other", architecture])).unwrap();
            }
        }
        assert_eq!(
            safetensors_architectures().count(),
            ardana_candle::ARCHITECTURES.len() - 1
        );
        for refused in [names(&["Qwen3_5ForConditionalGeneration"]), Vec::new()] {
            let err = candle_writes("a checkpoint", &refused)
                .expect_err("an architecture Candle lacks accepted")
                .to_string();
            assert!(err.ends_with("; pass --format gguf"), "{err}");
            assert!(
                err.contains(&safetensors_architectures().collect::<Vec<_>>().join(", ")),
                "{err}"
            );
        }
    }
}
