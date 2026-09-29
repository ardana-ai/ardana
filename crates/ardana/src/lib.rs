//! The `ardana` commands. `main.rs` only parses arguments and reports errors.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use ardana_api::{SystemOneRequest, SystemOneResponse, human_size};
use ardana_core::{DecideError, Decider, Layout, Limits, LoadOptions, LoadedModel, Runtimes};
use ardana_registry::library::library;
use ardana_registry::{LayoutKind, Named, PullOptions, Registry, ResolvedModel};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokenizers::Tokenizer;

#[derive(Debug, Parser)]
#[command(
    name = "ardana",
    version,
    about = "Pull, run and serve System 1 decision models locally"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Add a model to the registry (`$ARDANA_HOME/models.toml`): a library model, `hf.co/`, `ollama:` or a local
    /// GGUF.
    Pull(PullArgs),
    /// List the registry's models.
    List,
    /// Print a model's registry entry and profile as JSON.
    Show {
        /// The model name.
        name: String,
    },
    /// Remove a model from the registry; its weights and tokenizer stay where they are.
    Rm {
        /// The model name.
        name: String,
    },
    /// Answer one `/v1/systemone` request with a registry model (a library model is pulled first), or with explicit
    /// model files, and print the response as JSON.
    Run(RunArgs),
    /// Serve the registry's models, and the library's on first use, over the HTTP API and the playground:
    /// `POST /v1/systemone`, `GET /v1/models`, `GET /health`.
    Serve(ardana_server::ServeArgs),
}

#[derive(Debug, Args)]
pub struct PullArgs {
    /// A library model `<name>[:<quant>]` (decider-2b, decider-4b, qwen3.5-0.8b, smollm3-3b),
    /// `hf.co/<org>/<repo>[:<quant>]` (default quant Q4_K_M, matched case-insensitively),
    /// `hf.co/<org>/<repo>:<file>.gguf`, `ollama:[<namespace>/]<name>[:<tag>]` or a local GGUF path.
    pub reference: String,
    /// The tokenizer: `hf.co/<org>/<repo>` or a local `tokenizer.json`; default the weights' own repository or
    /// directory.
    #[arg(long)]
    pub tokenizer: Option<String>,
    /// The registry name; default the library name, the repository name lowercased without `-GGUF`, or the Ollama
    /// model name.
    #[arg(long)]
    pub name: Option<String>,
    /// The prompt layout; default `plain` for a model with `decider_config.json`, else `chat`.
    #[arg(long, value_enum)]
    pub layout: Option<LayoutArg>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A registry model (see `ardana list`) or a library model, pulled first; instead of `--gguf` and
    /// `--tokenizer`.
    #[arg(conflicts_with_all = ["gguf", "tokenizer", "config", "layout"])]
    pub name: Option<String>,
    /// The GGUF weights.
    #[arg(long, required_unless_present = "name")]
    pub gguf: Option<PathBuf>,
    /// The Hugging Face `tokenizer.json` the weights were converted with; the chat layout reads the chat template
    /// next to it (`chat_template.jinja`, else `tokenizer_config.json#chat_template`).
    #[arg(long, required_unless_present = "name")]
    pub tokenizer: Option<PathBuf>,
    /// The model's `decider_config.json`; without it the model is read with the stock profile (temperature 1.0,
    /// listwise score levels).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// The prompt layout: `plain` for decider-format models, `chat` for stock instruct models read zero-shot.
    #[arg(long, value_enum, default_value_t = LayoutArg::Plain)]
    pub layout: LayoutArg,
    /// A `/v1/systemone` request body.
    #[arg(long)]
    pub request: PathBuf,
    /// Layers offloaded to the GPU: -1 all, 0 none (CPU).
    #[arg(long, default_value_t = LoadOptions::default().gpu_layers, allow_negative_numbers = true)]
    pub gpu_layers: i32,
    /// The context window in tokens; a longer prompt row is refused, never truncated.
    #[arg(long, default_value_t = LoadOptions::default().n_ctx)]
    pub n_ctx: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LayoutArg {
    Plain,
    Chat,
}

impl From<LayoutArg> for LayoutKind {
    fn from(layout: LayoutArg) -> Self {
        match layout {
            LayoutArg::Plain => LayoutKind::Plain,
            LayoutArg::Chat => LayoutKind::Chat,
        }
    }
}

/// The name a model without `decider_config.json` reports (decider `serve.py#apply_config({})`).
const STOCK_NAME: &str = "decider-dev";

/// The runtimes this binary is built with.
pub fn runtimes() -> Runtimes {
    Runtimes(vec![Box::new(ardana_llama::LlamaRuntime)])
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Pull(args) => pull(&args),
        Command::List => {
            print!("{}", list(&Registry::open_default()?));
            Ok(())
        }
        Command::Show { name } => {
            let registry = Registry::open_default()?;
            println!("{}", serde_json::to_string_pretty(registry.entry(&name)?)?);
            Ok(())
        }
        Command::Rm { name } => {
            let mut registry = Registry::open_default()?;
            let removed = registry.remove(&name)?;
            registry.save()?;
            println!(
                "removed {} from {}; {} is untouched",
                removed.name,
                registry.path().display(),
                removed.weights.display()
            );
            Ok(())
        }
        Command::Run(args) => {
            let response = match &args.name {
                Some(name) => run_named(name, &args)?,
                None => run_files(&args)?,
            };
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        Command::Serve(args) => serve(&args),
    }
}

/// `ardana serve`: the registry's models behind the API until Ctrl-C or SIGTERM.
fn serve(args: &ardana_server::ServeArgs) -> Result<()> {
    let registry = Registry::open_default()?;
    let names = registry.names();
    let models = Arc::new(ardana_server::Models::new(
        registry,
        runtimes(),
        args.model_options(),
    )?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(async {
        let addr = args.addr();
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("binding {addr}"))?;
        let local = listener.local_addr().context("reading the bound address")?;
        let served = if names.is_empty() {
            format!(
                "no models pulled yet; the first request pulls {} or the library model it names",
                library().default
            )
        } else {
            format!("models {}", names.join(", "))
        };
        eprintln!(
            "ardana serve: listening on http://{local} ({served}{})",
            if args.api_key.is_some() {
                "; API key required"
            } else {
                ""
            }
        );
        let app = ardana_server::router(models.clone(), args.api_key.clone());
        ardana_server::serve(listener, app, models)
            .await
            .context("serving")
    })
}

/// `ardana pull`: resolve the library name or reference and record it in the registry.
fn pull(args: &PullArgs) -> Result<()> {
    let opts = PullOptions {
        name: args.name.clone(),
        tokenizer: args.tokenizer.clone(),
        layout: args.layout.map(LayoutKind::from),
        progress: true,
    };
    let (model, registry) = pull_into_registry(&args.reference, &opts)?;
    println!(
        "pulled {} ({}) into {}",
        model.name,
        model.source,
        registry.path().display()
    );
    Ok(())
}

/// Pulls `reference` (download progress on stderr) and records it in the registry, read again after the download.
fn pull_into_registry(reference: &str, opts: &PullOptions) -> Result<(ResolvedModel, Registry)> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    let model = runtime.block_on(ardana_registry::pull(reference, opts, &runtimes()))?;
    let mut registry = Registry::open_default()?;
    registry.insert(model.clone());
    registry.save()?;
    Ok((model, registry))
}

/// `ardana list`: one row per entry with name, source, format, size and layout.
fn list(registry: &Registry) -> String {
    let rows: Vec<[String; 5]> = registry
        .entries()
        .iter()
        .map(|m| {
            let (format, size) = match std::fs::metadata(&m.weights) {
                Ok(meta) => (
                    ardana_registry::weights_format(&m.weights).unwrap_or("unknown"),
                    human_size(meta.len()),
                ),
                Err(_) => ("missing", "-".to_string()),
            };
            let layout = match m.profile.layout {
                Layout::Plain => "plain",
                Layout::Chat { .. } => "chat",
            };
            [
                m.name.clone(),
                m.source.clone(),
                format.to_string(),
                size,
                layout.to_string(),
            ]
        })
        .collect();
    let header = ["NAME", "SOURCE", "FORMAT", "SIZE", "LAYOUT"].map(String::from);
    let widths: Vec<usize> = (0..header.len())
        .map(|i| {
            std::iter::once(&header)
                .chain(&rows)
                .map(|r| r[i].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in std::iter::once(&header).chain(&rows) {
        let cells: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, &w)| format!("{cell:<w$}"))
            .collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    out
}

/// `ardana run <name>`: the registry entry's tokenizer, profile and weights; a library model not pulled yet is
/// pulled first, with its progress on stderr.
fn run_named(name: &str, args: &RunArgs) -> Result<SystemOneResponse> {
    let registry = Registry::open_default()?;
    let name = match registry.named(name)? {
        Named::Pulled(model) => model.name.clone(),
        Named::Library(pick) => {
            let size = pick
                .size()
                .map(|bytes| format!(", {}", human_size(bytes)))
                .unwrap_or_default();
            eprintln!("pulling {} ({}{size})", pick.name(), pick.reference());
            let opts = PullOptions {
                progress: true,
                ..PullOptions::default()
            };
            let (model, registry) = pull_into_registry(&pick.name(), &opts)?;
            eprintln!("pulled {} into {}", model.name, registry.path().display());
            model.name
        }
    };
    let model = Registry::open_default()?.resolve(&name)?;
    let tokenizer = ardana_registry::load_tokenizer(&model.tokenizer)?;
    let runtimes = runtimes();
    decide(tokenizer, model.profile.clone(), args, |opts| {
        Ok(model.load(&runtimes, opts)?)
    })
}

/// `ardana run` on explicit files: the profile from `--config` and `--layout`, then the weights.
fn run_files(args: &RunArgs) -> Result<SystemOneResponse> {
    let (Some(gguf), Some(tokenizer_path)) = (&args.gguf, &args.tokenizer) else {
        anyhow::bail!("ardana run needs a model name, or --gguf and --tokenizer");
    };
    let tokenizer = ardana_registry::load_tokenizer(tokenizer_path)?;
    let profile = ardana_registry::read_profile(
        tokenizer_path,
        &tokenizer,
        args.config.as_deref(),
        args.layout.into(),
        STOCK_NAME,
    )?;
    let runtimes = runtimes();
    decide(tokenizer, profile, args, |opts| {
        let runtime = runtimes
            .for_weights(gguf)
            .with_context(|| format!("no runtime can load {}", gguf.display()))?;
        runtime.load(gguf, opts)
    })
}

/// Plans the request (refusing invalid and oversized ones before any load), loads the model and answers.
fn decide(
    tokenizer: Tokenizer,
    profile: ardana_core::ModelProfile,
    args: &RunArgs,
    load: impl FnOnce(&LoadOptions) -> Result<Box<dyn LoadedModel>>,
) -> Result<SystemOneResponse> {
    let request: SystemOneRequest = serde_json::from_value(read_json(&args.request, "request")?)
        .with_context(|| format!("{} is not a /v1/systemone request", args.request.display()))?;
    let limits = Limits {
        context_window: Some(args.n_ctx as usize),
        ..Limits::default()
    };
    let decider = Decider::new(tokenizer, profile)
        .map_err(describe)?
        .with_limits(limits);
    let plan = decider.plan(&request).map_err(describe)?;
    let mut model = load(&LoadOptions {
        n_ctx: args.n_ctx,
        gpu_layers: args.gpu_layers,
    })?;
    decider.run(model.as_mut(), &plan).map_err(describe)
}

fn read_json(path: &Path, what: &str) -> Result<serde_json::Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {what} {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {what} {}", path.display()))
}

/// A decide error as a CLI message, naming the offending field of an invalid request.
fn describe(err: DecideError) -> anyhow::Error {
    match err {
        DecideError::Invalid { loc, msg } => {
            anyhow::anyhow!("invalid request at {}: {msg}", loc.join("."))
        }
        DecideError::Capacity(msg) => anyhow::anyhow!("request too large: {msg}"),
        DecideError::Runtime(err) => err,
    }
}
