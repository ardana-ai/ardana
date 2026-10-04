//! The `ardana` commands, shaped like Ollama's: `serve`, `run`, `pull`, `list`, `show`, `ps`, `rm`. `main.rs` only
//! parses arguments and reports errors.

mod ask;
mod help;
mod render;

use std::io::{IsTerminal, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ardana_api::{SystemOneRequest, SystemOneResponse, human_size};
use ardana_core::{DecideError, Decider, Layout, Limits, LoadOptions, LoadedModel, Runtimes};
use ardana_registry::library::{LibraryPick, library};
use ardana_registry::{LayoutKind, Named, PullOptions, Registry, ResolvedModel};
use clap::{ArgAction, ArgMatches, Args, Parser, Subcommand, ValueEnum};
use serde_json::json;
use tokenizers::Tokenizer;

#[derive(Debug, Parser)]
#[command(
    name = "ardana",
    version,
    disable_version_flag = true,
    arg_required_else_help = true,
    about = "Run System 1 decision models locally",
    long_about = "Run System 1 decision models locally.\n\n\
                  A decision is one forward pass: the model reads a state once and answers typed questions \
                  (noul, choice, score) with calibrated probabilities.",
    after_help = help::main(),
)]
pub struct Cli {
    /// Print the version
    #[arg(short = 'v', long, action = ArgAction::Version)]
    version: Option<bool>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the API and the playground
    #[command(visible_alias = "start", after_help = help::serve())]
    Serve(ardana_server::ServeArgs),
    /// Ask a model questions about a state
    #[command(after_help = help::run())]
    Run(RunArgs),
    /// Download a model
    #[command(after_help = help::pull())]
    Pull(PullArgs),
    /// List pulled models
    #[command(visible_alias = "ls", after_help = help::list())]
    List,
    /// Show information for a model
    #[command(after_help = help::show())]
    Show(ShowArgs),
    /// List the models a running server has loaded
    #[command(after_help = help::ps())]
    Ps(PsArgs),
    /// Remove models
    #[command(after_help = help::rm())]
    Rm(RmArgs),
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A pulled model, or a library model (downloaded first)
    #[arg(value_name = "MODEL", required_unless_present = "gguf")]
    pub name: Option<String>,
    /// What the questions are about, as text or JSON; read from stdin when left out
    #[arg(value_name = "STATE")]
    pub state: Option<String>,
    /// A yes/no question; the answer is the probability of yes
    #[arg(long, value_name = "QUESTION", help_heading = "Questions")]
    pub noul: Vec<String>,
    /// A question, then two or more options to choose from
    #[arg(
        long,
        num_args = 3..,
        value_names = ["QUESTION", "OPTION"],
        help_heading = "Questions"
    )]
    pub choice: Vec<String>,
    /// A question, then 2 to 10 levels, lowest first
    #[arg(
        long,
        num_args = 3..=11,
        value_names = ["QUESTION", "LEVEL"],
        help_heading = "Questions"
    )]
    pub score: Vec<String>,
    /// A whole /v1/systemone request body instead: a JSON file, or - for stdin
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["state", "noul", "choice", "score"],
        help_heading = "Questions"
    )]
    pub request: Option<PathBuf>,
    /// Print the API response as JSON
    #[arg(long, help_heading = "Output")]
    pub json: bool,
    /// Also print the model, token counts and timings (on stderr)
    #[arg(long, help_heading = "Output")]
    pub verbose: bool,
    /// Run a GGUF file directly instead of a named model
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with = "name",
        requires = "tokenizer",
        help_heading = "Model files"
    )]
    pub gguf: Option<PathBuf>,
    /// The tokenizer.json the GGUF was converted with; a chat template is read next to it
    #[arg(
        long,
        value_name = "FILE",
        requires = "gguf",
        help_heading = "Model files"
    )]
    pub tokenizer: Option<PathBuf>,
    /// The model's decider_config.json; without it the stock profile is used
    #[arg(
        long,
        value_name = "FILE",
        requires = "gguf",
        help_heading = "Model files"
    )]
    pub config: Option<PathBuf>,
    /// plain for decider models, chat for instruct models read zero-shot
    #[arg(
        long,
        value_enum,
        default_value_t = LayoutArg::Plain,
        requires = "gguf",
        help_heading = "Model files"
    )]
    pub layout: LayoutArg,
    /// Layers offloaded to the GPU: -1 all, 0 none
    #[arg(
        long,
        default_value_t = LoadOptions::default().gpu_layers,
        allow_negative_numbers = true,
        help_heading = "Runtime"
    )]
    pub gpu_layers: i32,
    /// The context window in tokens; a longer prompt is refused, never truncated
    #[arg(long, default_value_t = LoadOptions::default().n_ctx, help_heading = "Runtime")]
    pub n_ctx: u32,
}

#[derive(Debug, Args)]
pub struct PullArgs {
    /// A library model, hf.co/<org>/<repo>[:<quant or file.gguf>], ollama:<name>[:<tag>] or a GGUF path
    #[arg(value_name = "MODEL")]
    pub reference: String,
    /// The tokenizer, when the weights come without one: hf.co/<org>/<repo> or a tokenizer.json
    #[arg(long, value_name = "TOKENIZER")]
    pub tokenizer: Option<String>,
    /// The name to pull it as; default the library name or the repository name
    #[arg(long)]
    pub name: Option<String>,
    /// plain for decider models, chat for instruct models; default plain when decider_config.json is present
    #[arg(long, value_enum)]
    pub layout: Option<LayoutArg>,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// A pulled model or a library model
    #[arg(value_name = "MODEL")]
    pub name: String,
    /// Print the registry entry as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct PsArgs {
    /// The server's address
    #[arg(
        long,
        env = "ARDANA_HOST",
        hide_env_values = true,
        default_value = "127.0.0.1"
    )]
    pub host: String,
    /// The server's port
    #[arg(
        long,
        env = "ARDANA_PORT",
        hide_env_values = true,
        default_value_t = 8000
    )]
    pub port: u16,
}

#[derive(Debug, Args)]
pub struct RmArgs {
    /// The models to remove
    #[arg(value_name = "MODEL", required = true)]
    pub names: Vec<String>,
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

/// Runs the command; `matches` are the ones `cli` was read from.
pub fn run(cli: Cli, matches: &ArgMatches) -> Result<()> {
    match cli.command {
        Command::Serve(args) => serve(&args),
        Command::Run(args) => {
            let asked = matches
                .subcommand_matches("run")
                .map(ask::Asked::from_matches)
                .unwrap_or_default();
            run_model(&args, &asked)
        }
        Command::Pull(args) => pull(&args),
        Command::List => list(),
        Command::Show(args) => show(&args),
        Command::Ps(args) => ps(&args),
        Command::Rm(args) => rm(&args),
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
        eprintln!("ardana serve: the playground is at http://{local}/");
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
    let (model, _) = pull_into_registry(&args.reference, &opts)?;
    println!("pulled {} ({}, {})", model.name, model.source, size(&model));
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

/// The weights' size, or `missing` when the file is gone.
fn size(model: &ResolvedModel) -> String {
    std::fs::metadata(&model.weights)
        .map_or_else(|_| "missing".to_string(), |m| human_size(m.len()))
}

/// `ardana list`: the pulled models, in pull order.
fn list() -> Result<()> {
    let registry = Registry::open_default()?;
    let today = render::today();
    let rows: Vec<Vec<String>> = registry
        .entries()
        .iter()
        .map(|m| {
            vec![
                m.name.clone(),
                size(m),
                render::ago(&m.pulled_at, today),
                m.source.clone(),
            ]
        })
        .collect();
    print!(
        "{}",
        render::table(&["NAME", "SIZE", "PULLED", "SOURCE"], &rows)
    );
    if rows.is_empty() {
        let library = library();
        eprintln!(
            "no models pulled yet; try `ardana pull {}` (library: {})",
            library.default,
            library.names().join(", ")
        );
    }
    Ok(())
}

/// `ardana show`: a pulled model's entry and profile, or a library model's download.
fn show(args: &ShowArgs) -> Result<()> {
    let registry = Registry::open_default()?;
    match registry.named(&args.name)? {
        Named::Pulled(model) if args.json => println!("{}", serde_json::to_string_pretty(model)?),
        Named::Pulled(model) => print!("{}", show_pulled(model)),
        Named::Library(pick) if args.json => {
            let entry = json!({
                "name": pick.name(),
                "source": pick.reference(),
                "size": pick.size(),
                "release_date": pick.model.release_date,
                "pulled": false,
            });
            println!("{}", serde_json::to_string_pretty(&entry)?);
        }
        Named::Library(pick) => print!("{}", show_library(&pick)),
    }
    Ok(())
}

fn show_pulled(model: &ResolvedModel) -> String {
    let profile = &model.profile;
    let layout = match profile.layout {
        Layout::Plain => "plain (decider format)",
        Layout::Chat { .. } => "chat (instruct model read zero-shot)",
    };
    let mut about = vec![
        ("name", model.name.clone()),
        ("source", model.source.clone()),
        ("size", size(model)),
        ("profile", profile.name.clone()),
        ("layout", layout.to_string()),
    ];
    if let Some(date) = &profile.release_date {
        about.push(("released", date.clone()));
    }
    about.push((
        "pulled",
        format!(
            "{} ({})",
            model.pulled_at,
            render::ago(&model.pulled_at, render::today())
        ),
    ));
    about.push(("runtime", model.runtime.clone()));
    let mut calibration: Vec<(&str, String)> = profile
        .effective_temperatures()
        .into_iter()
        .map(|(kind, t)| {
            (
                kind.as_str(),
                format!("temperature {}", ardana_api::format::raw(t)),
            )
        })
        .collect();
    calibration.push((
        "score levels",
        if profile.isolated_levels {
            "isolated (one yes/no row per level)"
        } else {
            "listwise"
        }
        .to_string(),
    ));
    let files = vec![
        ("weights", model.weights.display().to_string()),
        ("tokenizer", model.tokenizer.display().to_string()),
    ];
    render::sections(&[
        ("Model", about),
        ("Calibration", calibration),
        ("Files", files),
    ])
}

fn show_library(pick: &LibraryPick) -> String {
    let mut about = vec![("name", pick.name()), ("source", pick.reference())];
    if let Some(bytes) = pick.size() {
        about.push(("size", format!("{} to download", human_size(bytes))));
    }
    about.push(("released", pick.model.release_date.clone()));
    about.push((
        "status",
        format!(
            "not pulled yet; `ardana pull {}` or its first run downloads it",
            pick.name()
        ),
    ));
    render::sections(&[("Model", about)])
}

/// `ardana ps`: the models a running `ardana serve` has loaded, from its `/health`.
fn ps(args: &PsArgs) -> Result<()> {
    let health = get_json(&args.host, args.port, "/health")?;
    let registry = Registry::open_default()?;
    let rows: Vec<Vec<String>> = health["x_loaded"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str())
        .map(|name| match registry.entry(name) {
            Ok(model) => vec![name.to_string(), size(model), model.source.clone()],
            Err(_) => vec![name.to_string(), "-".to_string(), "-".to_string()],
        })
        .collect();
    print!("{}", render::table(&["NAME", "SIZE", "SOURCE"], &rows));
    if rows.is_empty() {
        eprintln!("no models loaded; a model loads on its first request");
    }
    Ok(())
}

/// `GET <path>` from the server at `host:port`, read as JSON.
fn get_json(host: &str, port: u16, path: &str) -> Result<serde_json::Value> {
    let url = format!("http://{host}:{port}");
    let unreachable = || format!("no ardana serve answers at {url}; start one with `ardana serve`");
    let addr = (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .with_context(unreachable)?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(2)).with_context(unreachable)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .with_context(|| format!("reading {url}{path}"))?;
    let (head, body) = reply.split_once("\r\n\r\n").unwrap_or((&reply, ""));
    let status = head.split_whitespace().nth(1).unwrap_or("?");
    if status != "200" {
        bail!("{url}{path} answered {status}");
    }
    serde_json::from_str(body).with_context(|| format!("{url}{path} did not answer JSON"))
}

/// `ardana rm`: the entries only, every name checked before any is removed.
fn rm(args: &RmArgs) -> Result<()> {
    let mut registry = Registry::open_default()?;
    let mut names: Vec<&str> = Vec::new();
    for name in &args.names {
        registry.entry(name)?;
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    let mut removed = Vec::new();
    for name in names {
        removed.push(registry.remove(name)?);
    }
    registry.save()?;
    for model in removed {
        let kept = if model.source.starts_with("hf.co/") {
            "its weights stay in the Hugging Face cache".to_string()
        } else if model.source.starts_with("ollama:") {
            "its weights stay in the Ollama store".to_string()
        } else {
            format!("its weights stay at {}", model.weights.display())
        };
        println!("removed {} ({kept})", model.name);
    }
    Ok(())
}

/// `ardana run`: builds the request, answers it and prints the answers (or the response as JSON).
fn run_model(args: &RunArgs, asked: &ask::Asked) -> Result<()> {
    let model = args.name.as_deref().unwrap_or("decider-2b");
    let request = match &args.request {
        Some(path) => read_request(path)?,
        None => {
            if asked.is_empty() {
                bail!(
                    "ask at least one question with --noul, --choice or --score, for example:\n  \
                     ardana run {model} \"My card was charged twice.\" --noul \"Does the customer ask for a refund?\""
                );
            }
            let state = read_state(args.state.as_deref(), model)?;
            asked.request(&state)
        }
    };
    let started = Instant::now();
    let answered = match &args.name {
        Some(name) => run_named(name, args, &request)?,
        None => run_files(args, &request)?,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&answered.response)?);
    } else {
        print!("{}", render::answers(&request, &answered.response));
    }
    if args.verbose {
        eprint!(
            "{}",
            render::verbose(
                &answered.response,
                answered.load,
                answered.answer,
                started.elapsed()
            )
        );
    }
    Ok(())
}

/// The state: the argument, else stdin when it is piped.
fn read_state(arg: Option<&str>, model: &str) -> Result<String> {
    if let Some(state) = arg {
        return Ok(state.to_string());
    }
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        bail!(
            "give the state the questions are about, as an argument or on stdin, for example:\n  \
             ardana run {model} \"My card was charged twice.\" --noul \"Does the customer ask for a refund?\"\n  \
             cat ticket.txt | ardana run {model} --noul \"Does the customer ask for a refund?\""
        );
    }
    let mut text = String::new();
    stdin
        .lock()
        .read_to_string(&mut text)
        .context("reading the state from stdin")?;
    let text = text.trim_end_matches(['\n', '\r']);
    if text.trim().is_empty() {
        bail!("stdin held no state; give it as an argument or pipe it in");
    }
    Ok(text.to_string())
}

/// A `/v1/systemone` request body from a file, or from stdin for `-`.
fn read_request(path: &Path) -> Result<SystemOneRequest> {
    let (text, what) = if path.as_os_str() == "-" {
        let mut text = String::new();
        std::io::stdin()
            .lock()
            .read_to_string(&mut text)
            .context("reading the request from stdin")?;
        (text, "stdin".to_string())
    } else {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading request {}", path.display()))?;
        (text, path.display().to_string())
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing request {what}"))?;
    serde_json::from_value(value).with_context(|| format!("{what} is not a /v1/systemone request"))
}

/// A response and how long loading and answering took.
struct Answered {
    response: SystemOneResponse,
    load: Duration,
    answer: Duration,
}

/// `ardana run <name>`: the registry entry's tokenizer, profile and weights; a library model not pulled yet is
/// pulled first, with its progress on stderr.
fn run_named(name: &str, args: &RunArgs, request: &SystemOneRequest) -> Result<Answered> {
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
            let (model, _) = pull_into_registry(&pick.name(), &opts)?;
            eprintln!("pulled {}", model.name);
            model.name
        }
    };
    let model = Registry::open_default()?.resolve(&name)?;
    let tokenizer = ardana_registry::load_tokenizer(&model.tokenizer)?;
    let runtimes = runtimes();
    decide(tokenizer, model.profile.clone(), args, request, |opts| {
        Ok(model.load(&runtimes, opts)?)
    })
}

/// `ardana run --gguf`: the profile from `--config` and `--layout`, then the weights.
fn run_files(args: &RunArgs, request: &SystemOneRequest) -> Result<Answered> {
    let (Some(gguf), Some(tokenizer_path)) = (&args.gguf, &args.tokenizer) else {
        bail!("ardana run needs a model name, or --gguf and --tokenizer");
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
    decide(tokenizer, profile, args, request, |opts| {
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
    request: &SystemOneRequest,
    load: impl FnOnce(&LoadOptions) -> Result<Box<dyn LoadedModel>>,
) -> Result<Answered> {
    let limits = Limits {
        context_window: Some(args.n_ctx as usize),
        ..Limits::default()
    };
    let decider = Decider::new(tokenizer, profile)
        .map_err(describe)?
        .with_limits(limits);
    let plan = decider.plan(request).map_err(describe)?;
    let loading = Instant::now();
    let mut model = load(&LoadOptions {
        n_ctx: args.n_ctx,
        gpu_layers: args.gpu_layers,
    })?;
    let load = loading.elapsed();
    let answering = Instant::now();
    let response = decider.run(model.as_mut(), &plan).map_err(describe)?;
    Ok(Answered {
        response,
        load,
        answer: answering.elapsed(),
    })
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_has_help_after_its_options() {
        let mut cli = Cli::command();
        for sub in cli.get_subcommands_mut() {
            let name = sub.get_name().to_string();
            if name == "help" {
                continue;
            }
            let help = sub.render_long_help().to_string();
            assert!(
                help.contains("Examples:") || name == "list" || name == "ps",
                "`ardana {name} --help` shows examples:\n{help}"
            );
        }
    }

    #[test]
    fn run_keeps_the_question_order() {
        let matches = Cli::command()
            .try_get_matches_from([
                "ardana",
                "run",
                "decider-2b",
                "state",
                "--score",
                "How upset?",
                "calm",
                "furious",
                "--noul",
                "Refund?",
                "--choice",
                "Which team?",
                "billing",
                "sales",
                "--noul",
                "Order number?",
            ])
            .unwrap();
        let asked = ask::Asked::from_matches(matches.subcommand_matches("run").unwrap());
        assert_eq!(
            asked.order,
            [
                (ask::Kind::Score, 0),
                (ask::Kind::Noul, 0),
                (ask::Kind::Choice, 0),
                (ask::Kind::Noul, 1)
            ]
        );
        let cli = <Cli as clap::FromArgMatches>::from_arg_matches(&matches).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("run")
        };
        assert_eq!(args.state.as_deref(), Some("state"));
        assert_eq!(asked.choice, [vec!["Which team?", "billing", "sales"]]);
        assert_eq!(asked.noul, ["Refund?", "Order number?"]);
        assert_eq!(args.choice, ["Which team?", "billing", "sales"]);
    }

    #[test]
    fn run_refuses_ambiguous_input() {
        let parse = |args: &[&str]| {
            Cli::command()
                .try_get_matches_from(std::iter::once("ardana").chain(args.iter().copied()))
                .map(|_| ())
                .map_err(|err| err.kind())
        };
        use clap::error::ErrorKind;
        // A choice needs two options, a score two levels.
        assert_eq!(
            parse(&["run", "m", "s", "--choice", "Q?", "only"]),
            Err(ErrorKind::TooFewValues)
        );
        assert_eq!(
            parse(&["run", "m", "s", "--score", "Q?", "low"]),
            Err(ErrorKind::TooFewValues)
        );
        // A whole request excludes inline questions.
        assert_eq!(
            parse(&["run", "m", "--request", "r.json", "--noul", "Q?"]),
            Err(ErrorKind::ArgumentConflict)
        );
        // Model files replace the name, and need a tokenizer.
        assert_eq!(
            parse(&["run", "m", "--gguf", "w.gguf", "--tokenizer", "t.json"]),
            Err(ErrorKind::ArgumentConflict)
        );
        assert_eq!(
            parse(&["run", "--gguf", "w.gguf", "--request", "r.json"]),
            Err(ErrorKind::MissingRequiredArgument)
        );
        assert_eq!(
            parse(&["run", "m", "--layout", "chat"]).map_err(|_| ()),
            Err(())
        );
        assert_eq!(parse(&["run", "m", "s", "--noul", "Q?"]), Ok(()));
        assert_eq!(parse(&["ls"]), Ok(()));
        assert_eq!(parse(&["start"]), Ok(()));
        assert_eq!(parse(&["rm", "a", "b"]), Ok(()));
        assert_eq!(parse(&["rm"]), Err(ErrorKind::MissingRequiredArgument));
    }
}
