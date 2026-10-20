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
pub use env::{LLAMA_CPP_COMMIT, PYTHON, TORCH, UV_VERSION};

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
    /// Let a checkpoint whose config.json or tokenizer_config.json has auto_map run the Python code it ships, as
    /// transformers' option of the same name does
    #[arg(long)]
    pub trust_remote_code: bool,
    /// Where to train: auto takes cuda, then mps, then cpu, as torch finds them; bfloat16 on cuda and mps (macOS 14 or
    /// later), float32 on cpu
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

/// `ardana finetune`, on Linux and on macOS with Apple Silicon (Q21).
pub fn finetune(args: &FinetuneArgs) -> Result<()> {
    runs_here(std::env::consts::OS, std::env::consts::ARCH)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(run(args))
}

/// Refuses a machine, its OS and architecture as `std::env::consts` names them, that fine-tuning does not run on
/// (Q21): anything but Linux and macOS, and Intel macOS, which no [`TORCH`] wheel is built for.
fn runs_here(os: &str, arch: &str) -> Result<()> {
    match (os, arch) {
        ("linux", _) | ("macos", "aarch64") => Ok(()),
        ("macos", _) => bail!(
            "fine-tuning on macOS needs Apple Silicon; PyTorch {TORCH} ships no Intel macOS wheels"
        ),
        (os, _) => bail!("fine-tuning runs on Linux and macOS; this is {os}"),
    }
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
    warn_sparse(&finetune::sparse_options(&split.train));
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
    own_classes(&env, &work, &mut interrupt, &base).await?;
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

    let trained = text_model(&checkpoint, &work.dir)?;
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
    let script = work.dir.join("train.py");
    std::fs::write(&script, include_str!("train.py"))?;
    let mut trainer = python(&env, &work);
    trainer
        .arg("-I")
        .arg(&script)
        .arg(training.device.name())
        .arg("--model_name_or_path")
        .arg(&trained.dir)
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
    if let Some(mapping) = trained.key_mapping() {
        // TRL passes these to `from_pretrained`.
        trainer
            .arg("--model_init_kwargs")
            .arg(json!({ "key_mapping": mapping }).to_string());
    }
    env::run(&mut trainer, &mut interrupt, "the trainer (TRL sft)").await?;

    let merged = work.dir.join("merged");
    let script = work.dir.join("merge.py");
    std::fs::write(&script, include_str!("merge.py"))?;
    let mut merge = python(&env, &work);
    merge.arg("-I").arg(&script).args(merge_args(
        &trained,
        &adapter,
        &merged,
        dtype,
        args.trust_remote_code,
    ));
    env::run(&mut merge, &mut interrupt, "merging the adapter (merge.py)").await?;

    // The GGUF's file in the model's directory; a checkpoint is the directory itself.
    let file = match &quant {
        Some(quant) => {
            copy_tokenizer_files(&trained.dir, &merged, args.trust_remote_code)?;
            if let Some(architecture) = &trained.tower {
                name_for_converter(&merged, architecture)?;
            }
            let converted = work.dir.join(format!("{name}-float32.gguf"));
            let script = work.dir.join("convert.py");
            std::fs::write(&script, include_str!("convert.py"))?;
            let mut convert = python(&env, &work);
            // `convert.py` puts the converter's directory on the path, which `-I` leaves out with the work directory.
            convert
                .arg("-I")
                .arg(&script)
                .arg(env.converter())
                .arg(trained.dir.join(snapshot::CONFIG))
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
            write_checkpoint(&trained.dir, &merged, &out, dtype)?;
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

/// The directory of `model` when `ardana finetune` made it: `$ARDANA_HOME/models/<name>/` (absolute) in `home`, holding
/// its weights and a `finetune.json` (Q20, Q28); `None` for any other entry.
pub fn made(home: &Path, model: &ResolvedModel) -> Result<Option<PathBuf>> {
    let dir = std::path::absolute(home.join(MODELS_DIR).join(&model.name))?;
    Ok((model.weights.starts_with(&dir) && dir.join(RECORD).is_file()).then_some(dir))
}

/// The examples of a split, owned, for a thread of their own.
fn owned(examples: &[&Example]) -> Vec<Example> {
    examples.iter().map(|&example| example.clone()).collect()
}

/// Q14 for a base whose `config.json` has `auto_map` and no `--trust-remote-code` ([`Base::model_code`]): the run goes
/// on when the environment's transformers implements the config's `model_type` itself (`native.py`), and so loads the
/// model by its own classes, never running the code the checkpoint ships (phi-4:mini's Phi-3, nemotron-3-nano:4b's
/// NemotronH); it is refused, naming the flag, when only that code loads it (spark-x2.5's). Before any weight.
async fn own_classes(
    env: &env::Env,
    work: &Work,
    interrupt: &mut Interrupt,
    base: &Base,
) -> Result<()> {
    let Some(code) = &base.model_code else {
        return Ok(());
    };
    let config = work.dir.join("checkpoint-config.json");
    std::fs::write(&config, base.config.to_string())?;
    let script = work.dir.join("native.py");
    std::fs::write(&script, include_str!("native.py"))?;
    let mut cmd = python(env, work);
    cmd.arg("-I").arg(&script).arg(&config).kill_on_drop(true);
    let output = interrupt
        .or(async { cmd.output().await.context("starting native.py") })
        .await?;
    ensure!(
        output.status.success(),
        "asking transformers for the model's classes (native.py) failed ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let answer: Value =
        serde_json::from_slice(&output.stdout).context("reading native.py's answer")?;
    let model_type = answer["model_type"].as_str().unwrap_or("no model_type");
    if answer["native"].as_bool() == Some(true) {
        eprintln!(
            "{}: its config.json has auto_map, and transformers implements {model_type} itself, so the model loads \
             without the Python code it names ({code})",
            base.checkpoint
        );
        return Ok(());
    }
    bail!(
        "{}: its config.json has auto_map, and transformers implements no {model_type} itself, so it loads the model by \
         running the Python code it names ({code}); pass --trust-remote-code to run that code",
        base.checkpoint
    )
}

/// The text model the trainer and `merge.py` load: a checkpoint directory, and for a multimodal checkpoint's text
/// tower the checkpoint's own architecture (`*ForConditionalGeneration`).
#[derive(Debug)]
struct Trained {
    dir: PathBuf,
    tower: Option<String>,
}

impl Trained {
    /// The `key_mapping` transformers loads a text tower's tensors with, [`TEXT_TOWER`].
    fn key_mapping(&self) -> Option<Value> {
        self.tower.as_ref().map(|_| {
            Value::Object(
                TEXT_TOWER
                    .iter()
                    .map(|(from, to)| (from.to_string(), json!(to)))
                    .collect(),
            )
        })
    }
}

/// The checkpoint as the trainer and `merge.py` load it, the text model Ardana runs. A multimodal checkpoint (its
/// `config.json` names a `*ForConditionalGeneration` and holds a `text_config`) trains its text tower alone: `<work>/
/// text/` holds the text tower's `config.json` (the `text_config`, its architecture the `*ForCausalLM` of the same
/// name) and links to the checkpoint's files but its processor's, and transformers loads the text model through
/// [`TEXT_TOWER`], which finds its tensors under the multimodal checkpoint's `language_model` names. A checkpoint
/// whose `tokenizer_config.json` names a `processor_class` (every Gemma 3 and 4 names its multimodal processor, the
/// text-only gemma-3:1b's too) is read there through a copy of it without that key, for any checkpoint: TRL's
/// `AutoProcessor` would build that processor, which wants an image processor's files, instead of the tokenizer. Any
/// other checkpoint is read as it is.
fn text_model(checkpoint: &Path, work: &Path) -> Result<Trained> {
    const PROCESSOR_FILES: [&str; 3] = [
        "processor_config.json",
        "preprocessor_config.json",
        "video_preprocessor_config.json",
    ];
    let read = |path: &Path| -> Result<Value> {
        Ok(serde_json::from_str(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
        )?)
    };
    let config = read(&checkpoint.join(snapshot::CONFIG))?;
    let multimodal = config["architectures"][0]
        .as_str()
        .and_then(|arch| arch.strip_suffix("ForConditionalGeneration"))
        .zip(config["text_config"].as_object());
    let tokenizer_config = checkpoint.join(ardana_core::chat::CONFIG_FILE);
    let tokenizer_config = if tokenizer_config.is_file() {
        Some(read(&tokenizer_config)?).filter(|config| config.get("processor_class").is_some())
    } else {
        None
    };
    if multimodal.is_none() && tokenizer_config.is_none() {
        return Ok(Trained {
            dir: checkpoint.to_path_buf(),
            tower: None,
        });
    }
    let dir = work.join("text");
    std::fs::create_dir_all(&dir)?;
    let mut written = vec![];
    if let Some((prefix, text)) = multimodal {
        let mut text = text.clone();
        text.insert(
            "architectures".into(),
            json!([format!("{prefix}ForCausalLM")]),
        );
        std::fs::write(
            dir.join(snapshot::CONFIG),
            serde_json::to_string_pretty(&text)?,
        )?;
        written.push(snapshot::CONFIG);
    }
    if let Some(mut tokenizer_config) = tokenizer_config {
        tokenizer_config
            .as_object_mut()
            .context("tokenizer_config.json is no object")?
            .remove("processor_class");
        std::fs::write(
            dir.join(ardana_core::chat::CONFIG_FILE),
            serde_json::to_string_pretty(&tokenizer_config)?,
        )?;
        written.push(ardana_core::chat::CONFIG_FILE);
    }
    for entry in std::fs::read_dir(checkpoint)? {
        let path = entry?.path();
        let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        if !written.contains(&file) && !PROCESSOR_FILES.contains(&file) {
            link(&std::fs::canonicalize(&path)?, &dir.join(file))?;
        }
    }
    let tower = multimodal.map(|(prefix, _)| format!("{prefix}ForConditionalGeneration"));
    Ok(Trained { dir, tower })
}

/// Names the merged text tower in `merged`'s `config.json` as its multimodal checkpoint is named, `architecture`, for
/// llama.cpp's converter, which takes the first of `architectures`: it registers its text model under every multimodal
/// name it converts (`Qwen3_5ForConditionalGeneration`, `Gemma3…`, `Gemma4…`, `Gemma4UnifiedForConditionalGeneration`)
/// and not always under the `*ForCausalLM` (b11074 has no `Gemma4UnifiedForCausalLM`); where it has both they are one
/// class. The tensors keep the text model's names, whose `language_model.` it would drop anyway.
fn name_for_converter(merged: &Path, architecture: &str) -> Result<()> {
    let path = merged.join(snapshot::CONFIG);
    let mut config: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    config["architectures"] = json!([architecture]);
    std::fs::write(&path, serde_json::to_string_pretty(&config)? + "\n")?;
    Ok(())
}

/// transformers' `key_mapping` for a multimodal checkpoint's text tower: the text model's own tensor names (`model.`,
/// `lm_head.`) for those under `model.language_model.` (Qwen3.5, Gemma 4) and `language_model.model.` (Gemma 3).
/// transformers 5.18 maps Qwen3.5's itself (`conversion_mapping.py`, `qwen3_5_text`), not Gemma's: without this a
/// Gemma text tower loads none of its weights and trains from random ones, with a warning.
const TEXT_TOWER: [(&str, &str); 3] = [
    (r"^model\.language_model\.", "model."),
    (r"^language_model\.model\.", "model."),
    (r"^language_model\.lm_head\.", "lm_head."),
];

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

/// Warns about the options under 50 training examples (Q26): each one of a question labelled on at least 50 training
/// lines, and in one line how many questions are labelled on fewer, every option of which is under 50 (as is every
/// question a line asks of its own).
fn warn_sparse(sparse: &[finetune::Sparse]) {
    let min = finetune::MIN_OPTION_EXAMPLES;
    let (common, rare): (Vec<_>, Vec<_>) =
        sparse.iter().partition(|question| question.lines >= min);
    for question in common {
        for (option, examples) in &question.options {
            eprintln!(
                "warning: question {} (first labelled on line {}) has {examples} training examples of {option}, \
                 fewer than {min}",
                question.question, question.line
            );
        }
    }
    match rare.len() {
        0 => {}
        1 => eprintln!(
            "warning: 1 question is labelled on fewer than {min} training lines, so each of its options has fewer \
             than {min} examples"
        ),
        n => eprintln!(
            "warning: {n} questions are labelled on fewer than {min} training lines, so each of their options has \
             fewer than {min} examples"
        ),
    }
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
/// run's working directory; it writes no bytecode (`-B`), so the environment's files stay as they were built, and its
/// output unbuffered (`-u`), so the trainer's losses come as it logs them. Flags, not `PYTHON*` variables: every step
/// runs with `-I` or `-E`, which ignore them.
fn python(env: &env::Env, work: &Work) -> Command {
    let mut cmd = Command::new(env.python());
    cmd.args(["-B", "-u"])
        .current_dir(&work.dir)
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("HF_HOME", work.dir.join("hf"))
        .env("TMPDIR", &work.tmp);
    cmd
}

/// `merge.py`'s arguments: the checkpoint, the adapter, the merged model's directory and the dtype, then
/// `--trust-remote-code` under the flag alone, so the checkpoint's own code runs only when asked (Q14), and
/// `--key-mapping <JSON>` for a text tower ([`Trained`]).
fn merge_args(
    trained: &Trained,
    adapter: &Path,
    merged: &Path,
    dtype: &str,
    trust_remote_code: bool,
) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        trained.dir.as_os_str().into(),
        adapter.into(),
        merged.into(),
        dtype.into(),
    ];
    if trust_remote_code {
        args.push("--trust-remote-code".into());
    }
    if let Some(mapping) = trained.key_mapping() {
        args.push("--key-mapping".into());
        args.push(mapping.to_string().into());
    }
    args
}

/// Copies the checkpoint's files that `save_pretrained` does not write (the tokenizer's) into the merged model, which
/// the converter reads them from. Its Python files only under `--trust-remote-code`: the converter loads tokenizers
/// with `trust_remote_code=True`, so a `*.py` beside them would run (Q14).
fn copy_tokenizer_files(checkpoint: &Path, merged: &Path, trust_remote_code: bool) -> Result<()> {
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
            && (trust_remote_code || !file.ends_with(".py"))
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
    /// there bitsandbytes; PyPI's wheels (no `--torch-backend`) on macOS arm64; PyTorch's CPU wheels on Linux without
    /// the driver. Intel macOS, which no torch 2.14.1 wheel is built for, and Windows are refused first (Q21).
    #[test]
    fn torch_build_follows_the_machine() {
        use env::{BITSANDBYTES, PACKAGES, TorchBuild};
        assert_eq!(PACKAGES[0], format!("torch=={TORCH}"));
        for (os, arch, refused) in [
            (
                "macos",
                "x86_64",
                "fine-tuning on macOS needs Apple Silicon",
            ),
            (
                "windows",
                "x86_64",
                "fine-tuning runs on Linux and macOS; this is windows",
            ),
        ] {
            let err = runs_here(os, arch).expect_err("refused").to_string();
            assert!(err.starts_with(refused), "{arch} {os}: {err}");
        }
        for (os, arch, nvidia, build, backend) in [
            ("linux", "x86_64", true, TorchBuild::Cuda, Some("auto")),
            ("linux", "aarch64", true, TorchBuild::Cuda, Some("auto")),
            ("linux", "x86_64", false, TorchBuild::Cpu, Some("cpu")),
            ("linux", "aarch64", false, TorchBuild::Cpu, Some("cpu")),
            ("macos", "aarch64", false, TorchBuild::Pypi, None),
        ] {
            let machine = format!("{arch} {os}, nvidia-smi answering: {nvidia}");
            runs_here(os, arch).unwrap_or_else(|err| panic!("{machine}: {err}"));
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

    /// Q14: the merged model gets the checkpoint's tokenizer files and no weights, and its Python files only under
    /// `--trust-remote-code`, since the converter loads a tokenizer with `trust_remote_code=True`.
    #[test]
    fn merged_model_gets_python_only_when_trusted() {
        let dir = std::env::temp_dir().join(format!("ardana-copy-{}", std::process::id()));
        let checkpoint = dir.join("checkpoint");
        std::fs::create_dir_all(&checkpoint).unwrap();
        for file in [
            "tokenizer.json",
            "tokenizer_config.json",
            "tokenization_x.py",
            "model.safetensors",
            "model.safetensors.index.json",
        ] {
            std::fs::write(checkpoint.join(file), file).unwrap();
        }
        for (trusted, wanted) in [
            (false, &["tokenizer.json", "tokenizer_config.json"][..]),
            (
                true,
                &[
                    "tokenization_x.py",
                    "tokenizer.json",
                    "tokenizer_config.json",
                ][..],
            ),
        ] {
            let merged = dir.join(format!("merged-{trusted}"));
            std::fs::create_dir_all(&merged).unwrap();
            copy_tokenizer_files(&checkpoint, &merged, trusted).unwrap();
            let mut copied: Vec<String> = std::fs::read_dir(&merged)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            copied.sort();
            assert_eq!(copied, wanted, "--trust-remote-code: {trusted}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Q14, R3.5: `merge.py` takes `<checkpoint> <adapter> <out> <dtype>` and runs the checkpoint's own code only when
    /// `--trust-remote-code` follows them, which it does under the flag alone; a text tower's key mapping follows as
    /// `--key-mapping <JSON>`.
    #[test]
    fn merge_runs_remote_code_only_when_trusted() {
        let (adapter, merged) = (Path::new("a"), Path::new("m"));
        let tower = Trained {
            dir: PathBuf::from("c"),
            tower: Some("Gemma3ForConditionalGeneration".into()),
        };
        let mapping = tower.key_mapping().unwrap().to_string();
        for (tower, mapped) in [
            (None, &[][..]),
            (tower.tower, &["--key-mapping", mapping.as_str()][..]),
        ] {
            let trained = Trained {
                dir: PathBuf::from("c"),
                tower,
            };
            for trusted in [false, true] {
                let mut wanted = vec!["c", "a", "m", "float32"];
                if trusted {
                    wanted.push("--trust-remote-code");
                }
                wanted.extend(mapped);
                assert_eq!(
                    merge_args(&trained, adapter, merged, "float32", trusted),
                    wanted,
                    "--trust-remote-code: {trusted}"
                );
            }
        }
        let script = include_str!("merge.py");
        assert!(script.contains(r#""--trust-remote-code" in flags"#));
        assert!(script.contains("save_original_format=key_mapping is None"));
    }

    /// The text model the trainer reads: a plain checkpoint as it is; one whose `tokenizer_config.json` names a
    /// `processor_class` (gemma-3:1b's) through a copy without it; a multimodal one (Gemma 3's, Qwen3.5's) through its
    /// `text_config` as a `*ForCausalLM`, without its processor's files, loaded with [`TEXT_TOWER`].
    #[test]
    fn text_models_read_without_processors() {
        let root = std::env::temp_dir().join(format!("ardana-text-{}", std::process::id()));
        let write = |dir: &Path, file: &str, value: &Value| {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(file), value.to_string()).unwrap();
        };
        let read = |path: PathBuf| -> Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        let tokenizer = json!({"tokenizer_class": "GemmaTokenizer", "bos_token": "<bos>"});
        let mut with_processor = tokenizer.clone();
        with_processor["processor_class"] = json!("Gemma3Processor");
        let text = json!({"model_type": "gemma3_text", "hidden_size": 2560});
        let mapping: serde_json::Map<String, Value> = TEXT_TOWER
            .iter()
            .map(|(from, to)| (from.to_string(), json!(to)))
            .collect();
        for (case, architecture, tokenizer_config, multimodal) in [
            ("plain", "Qwen3ForCausalLM", &tokenizer, false),
            ("processor", "Gemma3ForCausalLM", &with_processor, false),
            (
                "gemma3",
                "Gemma3ForConditionalGeneration",
                &with_processor,
                true,
            ),
            (
                "qwen3_5",
                "Qwen3_5ForConditionalGeneration",
                &tokenizer,
                true,
            ),
        ] {
            let checkpoint = root.join(case).join("checkpoint");
            let mut config = json!({"architectures": [architecture]});
            if multimodal {
                config["text_config"] = text.clone();
                write(&checkpoint, "preprocessor_config.json", &json!({}));
                write(&checkpoint, "processor_config.json", &json!({}));
            }
            write(&checkpoint, "config.json", &config);
            write(&checkpoint, "tokenizer_config.json", tokenizer_config);
            write(&checkpoint, "model.safetensors", &json!("weights"));
            let work = root.join(case).join("work");
            let trained = text_model(&checkpoint, &work).unwrap();
            if case == "plain" {
                assert_eq!(trained.dir, checkpoint);
                assert!(trained.tower.is_none() && trained.key_mapping().is_none());
                continue;
            }
            assert_eq!(trained.dir, work.join("text"), "{case}");
            let mut files: Vec<String> = std::fs::read_dir(&trained.dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            files.sort();
            assert_eq!(
                files,
                ["config.json", "model.safetensors", "tokenizer_config.json"],
                "{case}"
            );
            assert_eq!(
                read(trained.dir.join("model.safetensors")),
                json!("weights"),
                "{case}"
            );
            assert_eq!(
                read(trained.dir.join("tokenizer_config.json")),
                tokenizer,
                "{case}"
            );
            let config = read(trained.dir.join("config.json"));
            if multimodal {
                let mut wanted = text.clone();
                wanted["architectures"] =
                    json!([architecture.replace("ForConditionalGeneration", "ForCausalLM")]);
                assert_eq!(config, wanted, "{case}");
                assert_eq!(trained.tower.as_deref(), Some(architecture), "{case}");
                assert_eq!(
                    trained.key_mapping(),
                    Some(Value::Object(mapping.clone())),
                    "{case}"
                );
            } else {
                assert_eq!(config, json!({"architectures": [architecture]}), "{case}");
                assert!(
                    trained.tower.is_none() && trained.key_mapping().is_none(),
                    "{case}"
                );
            }
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The merged text tower reaches llama.cpp's converter under its multimodal checkpoint's architecture, which the
    /// converter registers (b11074 has `Gemma4UnifiedForConditionalGeneration`, no `Gemma4UnifiedForCausalLM`), every
    /// other setting as `save_pretrained` wrote it.
    #[test]
    fn converter_reads_a_tower_under_its_checkpoints_name() {
        let merged = std::env::temp_dir().join(format!("ardana-merged-{}", std::process::id()));
        std::fs::create_dir_all(&merged).unwrap();
        let saved = json!({"architectures": ["Gemma4UnifiedForCausalLM"], "model_type": "gemma4_unified_text",
            "hidden_size": 3840, "dtype": "bfloat16"});
        std::fs::write(merged.join("config.json"), saved.to_string()).unwrap();
        name_for_converter(&merged, "Gemma4UnifiedForConditionalGeneration").unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(merged.join("config.json")).unwrap())
                .unwrap();
        let mut wanted = saved;
        wanted["architectures"] = json!(["Gemma4UnifiedForConditionalGeneration"]);
        assert_eq!(written, wanted);
        std::fs::remove_dir_all(&merged).unwrap();
    }

    /// Q24: every library size's default quant is one the linked llama.cpp writes, `llama-quantize`'s aliases among
    /// them (phi-4:14b's `q4_k`, Q4_K_M), so no run without `--quant` is refused on its base's own quant.
    #[test]
    fn library_quants_are_written() {
        let snapshot =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../ardana-registry/tests/data/models.json");
        let sizes = Library::from_file(&snapshot).unwrap().sizes();
        assert!(
            sizes
                .iter()
                .any(|pick| pick.name() == "phi-4:14b" && pick.quant.quant == "q4_k")
        );
        for pick in sizes {
            quant_is_written(&pick.quant.quant, &pick.name()).unwrap_or_else(|err| panic!("{err}"));
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
