//! The `ardana` commands. `main.rs` only parses arguments and reports errors.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ardana_api::SystemOneRequest;
use ardana_core::{
    DecideError, Decider, Layout, Limits, LoadOptions, Runtimes, from_decider_config,
};
use clap::{Args, Parser, Subcommand};
use tokenizers::Tokenizer;

#[derive(Debug, Parser)]
#[command(
    name = "ardana",
    version,
    about = "Serve System 1 decision models over a Jev-compatible API"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Answer one `/v1/systemone` request from explicit model files and print the response as JSON.
    Run(RunArgs),
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// The GGUF weights.
    #[arg(long)]
    pub gguf: PathBuf,
    /// The Hugging Face `tokenizer.json` the weights were converted with.
    #[arg(long)]
    pub tokenizer: PathBuf,
    /// The model's `decider_config.json`.
    #[arg(long)]
    pub config: PathBuf,
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

/// The runtimes this binary is built with.
pub fn runtimes() -> Runtimes {
    Runtimes(vec![Box::new(ardana_llama::LlamaRuntime)])
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Run(args) => {
            let response = run_files(&args)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
    }
}

fn read_json(path: &Path, what: &str) -> Result<serde_json::Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {what} {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {what} {}", path.display()))
}

/// `ardana run` on explicit files: load the profile, tokenizer and weights, then answer the request.
pub fn run_files(args: &RunArgs) -> Result<ardana_api::SystemOneResponse> {
    let cfg = read_json(&args.config, "config")?;
    let profile = from_decider_config(&cfg, Layout::Plain)
        .with_context(|| format!("reading {}", args.config.display()))?;
    let request: SystemOneRequest = serde_json::from_value(read_json(&args.request, "request")?)
        .with_context(|| format!("{} is not a /v1/systemone request", args.request.display()))?;
    let tokenizer = Tokenizer::from_file(&args.tokenizer)
        .map_err(|err| anyhow::anyhow!("loading {}: {err}", args.tokenizer.display()))?;
    let limits = Limits {
        context_window: Some(args.n_ctx as usize),
        ..Limits::default()
    };
    let decider = Decider::new(tokenizer, profile)
        .map_err(describe)?
        .with_limits(limits);
    let plan = decider.plan(&request).map_err(describe)?;

    let runtimes = runtimes();
    let runtime = runtimes
        .for_weights(&args.gguf)
        .with_context(|| format!("no runtime can load {}", args.gguf.display()))?;
    let opts = LoadOptions {
        n_ctx: args.n_ctx,
        gpu_layers: args.gpu_layers,
    };
    let mut model = runtime.load(&args.gguf, &opts)?;
    decider.run(model.as_mut(), &plan).map_err(describe)
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
