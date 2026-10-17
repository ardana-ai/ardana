//! The base `ardana finetune` trains from (Q6, Q15): every model `ardana` runs whose transformers checkpoint is known.
//! A library size trains from the checkpoint the library pins for it (a decider size's `source`, an instruct size's
//! `tokenizer`, which is the whole repository at a commit); an `hf.co/<org>/<repo>` safetensors checkpoint from itself;
//! an `hf.co/` GGUF reference, and a registry entry pulled from one, from the `base_model` its repository's card
//! (`README.md`) names when it names exactly one repository; `--checkpoint hf.co/<org>/<repo>` or `--checkpoint <dir>`
//! names it for any base, and without it an `ollama:` reference, a local GGUF file and a library size without a pin
//! (decider:4b) are refused, naming the flag. [`base`] reads what the fine-tune needs before any weight: the
//! checkpoint's listing and `config.json`, the tokenizer and profile `ardana pull` derives for the base; it refuses a
//! decision model (THX-01, Q17) and, without `--trust-remote-code`, a checkpoint whose `config.json` or
//! `tokenizer_config.json` has `auto_map` (Q14). [`Base::download`] then fetches the checkpoint into the hub cache.

use std::fmt;
use std::path::{Path, PathBuf};

use ardana_core::snapshot::CONFIG;
use ardana_core::{Layout, LayoutKind, ModelProfile, RepoPin, chat, thx01};
use serde::Serialize;

use crate::hub::{Hub, Snapshot};
use crate::library::{Library, LibraryPick};
use crate::refs::{self, HfFile, Ref};
use crate::{
    DECIDER_CONFIG, PullOptions, Registry, RegistryError, ResolvedModel, TOKENIZER_FILE,
    default_layout, hf_tokenizer, is_checkpoint, load_tokenizer, local_file, ollama, pick_gguf,
    read_json, read_profile, safetensors, tokenizer_ref,
};

/// A Hugging Face repository's model card, whose front matter names the repositories a model derives from.
pub const CARD: &str = "README.md";

/// What `ardana finetune` takes besides the base.
#[derive(Debug, Clone, Copy, Default)]
pub struct BaseOptions<'a> {
    /// `--checkpoint`: `hf.co/<org>/<repo>` (read at `main`) or a directory, the checkpoint to train from whatever the
    /// base names.
    pub checkpoint: Option<&'a str>,
    /// `--trust-remote-code`: a checkpoint whose `config.json` or `tokenizer_config.json` has `auto_map` may run the
    /// code it ships.
    pub trust_remote_code: bool,
    /// Print each download's progress to stderr.
    pub progress: bool,
}

/// A base as `ardana finetune` reads it before any weight.
#[derive(Debug, Clone)]
pub struct Base {
    /// The base as `finetune.json` and the report name it: a library size's canonical name, an entry's name, the
    /// reference as given (a local file's absolute path).
    pub name: String,
    /// The transformers checkpoint it trains from.
    pub checkpoint: Checkpoint,
    /// The checkpoint's `config.json#architectures`.
    pub architectures: Vec<String>,
    /// The checkpoint's `config.json`.
    pub config: serde_json::Value,
    /// The quant of the base's own GGUF, when the library or the reference names one.
    pub quant: Option<String>,
    /// The file name of the base's own GGUF, when it is one with a name of its own (not an Ollama blob).
    pub gguf: Option<String>,
    /// The size's license, else its family's; for any other base the `license` of its repository's card, else of the
    /// checkpoint's.
    pub license: Option<String>,
    /// The layout the base is read in.
    pub layout: LayoutKind,
    /// The `tokenizer.json` the base reads with, its chat template files beside it.
    pub tokenizer: PathBuf,
    /// The profile `ardana pull` derives for the base.
    pub profile: ModelProfile,
    /// How the base itself answers, for the report.
    pub model: BaseModel,
    /// The checkpoint's files.
    files: Files,
}

/// The checkpoint a fine-tune trains from, as `finetune.json` records it: `{"repo", "commit"}` or `{"path"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Checkpoint {
    /// A Hub repository at a commit.
    Hub(RepoPin),
    /// A directory holding `config.json` and its safetensors, absolute.
    Dir { path: PathBuf },
}

impl fmt::Display for Checkpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Checkpoint::Hub(pin) => write!(f, "{} at {}", pin.repo, pin.commit),
            Checkpoint::Dir { path } => write!(f, "{}", path.display()),
        }
    }
}

/// The base as the report reads it, the model `ardana run <base>` answers with.
#[derive(Debug, Clone)]
pub enum BaseModel {
    /// A library size: its entry when pulled, else pulled first under its canonical name.
    Library(Box<LibraryPick>),
    /// A registry entry, its files in place.
    Pulled(Box<ResolvedModel>),
    /// A reference, resolved as `ardana pull <reference>` with these options resolves it.
    Reference {
        reference: String,
        opts: PullOptions,
    },
}

/// A checkpoint's files: a repository commit's listing, or a directory.
#[derive(Debug, Clone)]
enum Files {
    Hub(Snapshot),
    Dir(PathBuf),
}

/// Root files of a checkpoint [`Base::download`] leaves out: weights in other formats than its safetensors.
const OTHER_WEIGHTS: [&str; 8] = [
    ".gguf",
    ".bin",
    ".pt",
    ".pth",
    ".onnx",
    ".h5",
    ".msgpack",
    ".safetensors",
];

/// The base `input` names: a size `library` holds, else an entry of `registry`, else a reference; read through the hub
/// from the environment (offline under `HF_HUB_OFFLINE`), with no weight downloaded.
pub async fn base(
    input: &str,
    library: &Library,
    registry: &Registry,
    opts: BaseOptions<'_>,
) -> Result<Base, RegistryError> {
    let hub = Hub::from_env(opts.progress)?;
    let named = match opts.checkpoint {
        Some(checkpoint) => Some(named_checkpoint(&hub, checkpoint).await?),
        None => None,
    };
    let pick = match library.find(input) {
        Ok(pick) => pick,
        Err(_) if registry.entry(input).is_ok() => None,
        Err(err) => return Err(err),
    };
    match pick {
        Some(pick) => library_base(&hub, pick, named, opts).await,
        None if registry.entry(input).is_ok() => {
            entry_base(&hub, registry.resolve(input)?, named, opts).await
        }
        None => reference_base(&hub, input, named, opts).await,
    }
}

/// A library size, its checkpoint the library's pin unless `--checkpoint` names one.
async fn library_base(
    hub: &Hub,
    pick: LibraryPick,
    named: Option<Files>,
    opts: BaseOptions<'_>,
) -> Result<Base, RegistryError> {
    let name = pick.name();
    let size = &pick.size;
    let files = match (named, size.source.as_ref().or(size.tokenizer.as_ref())) {
        (Some(files), _) => files,
        (None, Some(pin)) => {
            let (org, repo) = whole_repo(&pin.repo, "a checkpoint")?;
            Files::Hub(hub.snapshot(&org, &repo, Some(&pin.commit)).await?)
        }
        (None, None) => {
            return Err(unknown_checkpoint(
                name,
                "the library pins no transformers checkpoint for it to train from",
            ));
        }
    };
    let (checkpoint, model_config) = open(hub, &files, opts.trust_remote_code).await?;

    // The GGUF repository is read, at its commit, for what `ardana pull` takes from it besides the weights: its
    // tokenizer when the size pins none, and `decider_config.json` when the size names no layout (Q7).
    let gguf = if size.tokenizer.is_none() || size.layout.is_none() {
        let (org, repo) = whole_repo(&size.gguf.repo, "a GGUF repository")?;
        Some(hub.snapshot(&org, &repo, Some(&size.gguf.commit)).await?)
    } else {
        None
    };
    let config = match gguf
        .as_ref()
        .filter(|gguf| size.layout.is_none() && gguf.has(DECIDER_CONFIG))
    {
        Some(gguf) => Some(hub.file(gguf, DECIDER_CONFIG).await?),
        None => None,
    };
    let tokenizer = match &size.tokenizer {
        Some(pin) => tokenizer_ref(hub, &pin.repo, Some(&pin.commit)).await?,
        None => {
            let gguf = gguf
                .as_ref()
                .expect("a size without a tokenizer pin has its GGUF repository read");
            hf_tokenizer(hub, gguf).await?
        }
    };
    let layout = match size.layout {
        Some(layout) => layout,
        None => default_layout(config.as_deref())?,
    };
    let mut profile = read_profile(
        &tokenizer,
        &load_tokenizer(&tokenizer)?,
        config.as_deref(),
        layout,
        &name,
    )?;
    profile
        .release_date
        .get_or_insert_with(|| size.release_date.clone());
    Ok(Base {
        name,
        checkpoint,
        architectures: ardana_core::snapshot::architectures(&model_config),
        config: model_config,
        quant: Some(pick.quant.quant.clone()),
        gguf: Some(pick.quant.file.clone()),
        license: size.license.clone(),
        layout,
        tokenizer,
        profile,
        model: BaseModel::Library(Box::new(pick)),
        files,
    })
}

/// A registry entry: its tokenizer and profile as recorded, its checkpoint the one its source names.
async fn entry_base(
    hub: &Hub,
    entry: ResolvedModel,
    named: Option<Files>,
    opts: BaseOptions<'_>,
) -> Result<Base, RegistryError> {
    let source = Ref::parse(&entry.source);
    let (files, card) = match (named, &source) {
        (Some(files), _) => (files, Card::default()),
        (None, Ok(Ref::Hf { org, repo, file })) => {
            let snapshot = hub.snapshot(org, repo, None).await?;
            undecided(&snapshot.id(), snapshot.has(thx01::DECISION_CONFIG))?;
            if file.is_none() && is_checkpoint(&snapshot) {
                (Files::Hub(snapshot), Card::default())
            } else {
                let card = card(hub, &snapshot).await?;
                (card_checkpoint(hub, &snapshot, &card).await?, card)
            }
        }
        (None, _) => {
            return Err(unknown_checkpoint(
                format!("{} ({})", entry.name, entry.source),
                "its source names no transformers checkpoint",
            ));
        }
    };
    let (checkpoint, model_config) = open(hub, &files, opts.trust_remote_code).await?;
    let license = license(hub, card, &files).await?;
    let quant = match source {
        Ok(Ref::Hf {
            file: Some(HfFile::Quant(quant)),
            ..
        }) => Some(quant.to_ascii_lowercase()),
        _ => None,
    };
    let layout = match entry.profile.layout {
        Layout::Plain => LayoutKind::Plain,
        Layout::Chat { .. } => LayoutKind::Chat,
    };
    Ok(Base {
        name: entry.name.clone(),
        checkpoint,
        architectures: ardana_core::snapshot::architectures(&model_config),
        config: model_config,
        quant,
        gguf: entry
            .weights
            .is_file()
            .then(|| file_name(&entry.weights))
            .flatten(),
        license,
        layout,
        tokenizer: entry.tokenizer.clone(),
        profile: entry.profile.clone(),
        model: BaseModel::Pulled(Box::new(entry)),
        files,
    })
}

/// A reference: an `hf.co/` safetensors checkpoint trains from itself, an `hf.co/` GGUF reference from its card's
/// `base_model`; an `ollama:` reference and a local GGUF file only from `--checkpoint`. The tokenizer and profile are
/// those `ardana pull <reference>` derives, the checkpoint's tokenizer standing in when the weights come with none.
async fn reference_base(
    hub: &Hub,
    input: &str,
    named: Option<Files>,
    opts: BaseOptions<'_>,
) -> Result<Base, RegistryError> {
    let parsed = Ref::parse(input)?;
    let mut name = input.to_string();
    let mut card = Card::default();
    let mut gguf = None;
    let mut quant = None;
    let (files, config, own_tokenizer) = match &parsed {
        Ref::Hf { org, repo, file } => {
            let snapshot = hub.snapshot(org, repo, None).await?;
            undecided(&snapshot.id(), snapshot.has(thx01::DECISION_CONFIG))?;
            let own = file.is_none() && is_checkpoint(&snapshot);
            if !own {
                gguf = Some(pick_gguf(&snapshot, file.as_ref())?);
                card = self::card(hub, &snapshot).await?;
                if let Some(HfFile::Quant(named)) = file {
                    quant = Some(named.to_ascii_lowercase());
                }
            }
            let config = if snapshot.has(DECIDER_CONFIG) {
                Some(hub.file(&snapshot, DECIDER_CONFIG).await?)
            } else {
                None
            };
            let tokenizer = if snapshot.has(TOKENIZER_FILE) {
                Some(hf_tokenizer(hub, &snapshot).await?)
            } else {
                None
            };
            let files = match named {
                Some(files) => files,
                None if own => Files::Hub(snapshot),
                None => card_checkpoint(hub, &snapshot, &card).await?,
            };
            (files, config, tokenizer)
        }
        Ref::Ollama {
            namespace,
            name: model,
            tag,
        } => {
            let files = named.ok_or_else(|| {
                unknown_checkpoint(
                    input.to_string(),
                    "an Ollama model names no transformers checkpoint",
                )
            })?;
            ollama::blob(input, namespace, model, tag)?;
            (files, None, None)
        }
        Ref::Local(path) => {
            let weights = local_file(path)?;
            name = weights.display().to_string();
            let files = named.ok_or_else(|| {
                unknown_checkpoint(
                    name.clone(),
                    "a local GGUF file names no transformers checkpoint",
                )
            })?;
            gguf = file_name(&weights);
            let beside = |file: &str| {
                let path = weights.with_file_name(file);
                path.is_file().then_some(path)
            };
            (files, beside(DECIDER_CONFIG), beside(TOKENIZER_FILE))
        }
    };
    let (checkpoint, model_config) = open(hub, &files, opts.trust_remote_code).await?;
    let (tokenizer, pull_tokenizer) = match own_tokenizer {
        Some(path) => (path, None),
        None => match &checkpoint {
            Checkpoint::Hub(pin) => (
                tokenizer_ref(hub, &pin.repo, Some(&pin.commit)).await?,
                Some(pin.repo.clone()),
            ),
            Checkpoint::Dir { path } => (
                local_file(&path.join(TOKENIZER_FILE))?,
                Some(path.display().to_string()),
            ),
        },
    };
    let layout = default_layout(config.as_deref())?;
    let profile = read_profile(
        &tokenizer,
        &load_tokenizer(&tokenizer)?,
        config.as_deref(),
        layout,
        &parsed.default_name(),
    )?;
    Ok(Base {
        model: BaseModel::Reference {
            reference: name.clone(),
            opts: PullOptions {
                name: None,
                tokenizer: pull_tokenizer,
                layout: Some(layout),
                progress: opts.progress,
            },
        },
        name,
        checkpoint,
        architectures: ardana_core::snapshot::architectures(&model_config),
        config: model_config,
        quant,
        gguf,
        license: license(hub, card, &files).await?,
        layout,
        tokenizer,
        profile,
        files,
    })
}

/// The checkpoint `--checkpoint` names: a directory, else `hf.co/<org>/<repo>` listed at `main`.
async fn named_checkpoint(hub: &Hub, checkpoint: &str) -> Result<Files, RegistryError> {
    let path = Path::new(checkpoint);
    if path.is_dir() {
        let path = std::path::absolute(path).map_err(|err| RegistryError::Io {
            path: path.to_path_buf(),
            err,
        })?;
        return Ok(Files::Dir(path));
    }
    match Ref::parse(checkpoint) {
        Ok(Ref::Hf {
            org,
            repo,
            file: None,
        }) => Ok(Files::Hub(hub.snapshot(&org, &repo, None).await?)),
        _ => Err(RegistryError::Invalid {
            what: format!("--checkpoint {checkpoint}"),
            msg: format!(
                "a checkpoint is a whole repository, {}<org>/<repo>, or a directory holding {CONFIG} and its \
                 safetensors",
                refs::HF_PREFIX
            ),
        }),
    }
}

/// `files` as a checkpoint to train from, with its `config.json`: no decision model (Q17), a `config.json` (fetched,
/// no weight) and its weights, and, unless `trust_remote_code`, no `auto_map` in its `config.json` or its
/// `tokenizer_config.json` (fetched too), which would run the code it ships (Q14, [`remote_code`]).
async fn open(
    hub: &Hub,
    files: &Files,
    trust_remote_code: bool,
) -> Result<(Checkpoint, serde_json::Value), RegistryError> {
    let (checkpoint, config, tokenizer_config, code) = match files {
        Files::Hub(snapshot) => {
            let id = snapshot.id();
            undecided(&id, snapshot.has(thx01::DECISION_CONFIG))?;
            if !snapshot.has(CONFIG) {
                return Err(unknown_checkpoint(
                    id,
                    &format!("it has no {CONFIG}, so it is no transformers checkpoint"),
                ));
            }
            let config = hub.file(snapshot, CONFIG).await?;
            let tokenizer_config = if snapshot.has(chat::CONFIG_FILE) {
                Some(hub.file(snapshot, chat::CONFIG_FILE).await?)
            } else {
                None
            };
            let code: Vec<String> = snapshot
                .files
                .iter()
                .filter(|file| !file.contains('/') && file.ends_with(".py"))
                .cloned()
                .collect();
            let pin = RepoPin {
                repo: id,
                commit: snapshot.commit().to_string(),
            };
            (Checkpoint::Hub(pin), config, tokenizer_config, code)
        }
        Files::Dir(dir) => {
            let what = dir.display().to_string();
            undecided(&what, dir.join(thx01::DECISION_CONFIG).is_file())?;
            let config = dir.join(CONFIG);
            if !config.is_file() {
                return Err(unknown_checkpoint(
                    what,
                    &format!("it holds no {CONFIG}, so it is no transformers checkpoint"),
                ));
            }
            ardana_core::snapshot::weight_files(dir).map_err(|msg| RegistryError::Invalid {
                what: what.clone(),
                msg,
            })?;
            let io = |err| RegistryError::Io {
                path: dir.clone(),
                err,
            };
            let mut code = Vec::new();
            for entry in std::fs::read_dir(dir).map_err(io)? {
                let file = entry
                    .map_err(io)?
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                if file.ends_with(".py") {
                    code.push(file);
                }
            }
            code.sort();
            let tokenizer_config = Some(dir.join(chat::CONFIG_FILE)).filter(|path| path.is_file());
            (
                Checkpoint::Dir { path: dir.clone() },
                config,
                tokenizer_config,
                code,
            )
        }
    };
    let config = read_json(&config)?;
    let tokenizer_config = tokenizer_config.map(|path| read_json(&path)).transpose()?;
    if let Some(msg) = remote_code(&config, tokenizer_config.as_ref(), &code)
        && !trust_remote_code
    {
        return Err(RegistryError::Invalid {
            what: checkpoint.to_string(),
            msg,
        });
    }
    Ok((checkpoint, config))
}

/// Why a checkpoint runs Python code it ships, `code` its root `*.py` files, when it does: its `config.json` has
/// `auto_map`, so transformers loads the model by running that code, or its `tokenizer_config.json` has, so
/// transformers loads the tokenizer by it, and llama.cpp's converter loads every such tokenizer with
/// `trust_remote_code=True` (Q14). The reason ends naming `--trust-remote-code`.
fn remote_code(
    config: &serde_json::Value,
    tokenizer_config: Option<&serde_json::Value>,
    code: &[String],
) -> Option<String> {
    let shipped = if code.is_empty() {
        "code from other repositories".to_string()
    } else {
        code.join(", ")
    };
    let (file, what) = if config.get("auto_map").is_some() {
        (CONFIG, "it")
    } else if tokenizer_config.is_some_and(|config| config.get("auto_map").is_some()) {
        (chat::CONFIG_FILE, "its tokenizer")
    } else {
        return None;
    };
    Some(format!(
        "its {file} has auto_map, so transformers loads {what} by running the Python code it names ({shipped}); \
         pass --trust-remote-code to run that code"
    ))
}

/// The checkpoint the card of `snapshot` names: its one `base_model`, listed at `main`.
async fn card_checkpoint(
    hub: &Hub,
    snapshot: &Snapshot,
    card: &Card,
) -> Result<Files, RegistryError> {
    let id = snapshot.id();
    let one = match &card.base_models[..] {
        [one] => one,
        [] => {
            return Err(unknown_checkpoint(
                id,
                &format!("its card ({CARD}) names no base_model"),
            ));
        }
        many => {
            return Err(unknown_checkpoint(
                id,
                &format!(
                    "its card ({CARD}) names {} base models ({}), not one",
                    many.len(),
                    many.join(", ")
                ),
            ));
        }
    };
    match Ref::parse(&format!("{}{one}", refs::HF_PREFIX)) {
        Ok(Ref::Hf {
            org,
            repo,
            file: None,
        }) => Ok(Files::Hub(hub.snapshot(&org, &repo, None).await?)),
        _ => Err(unknown_checkpoint(
            id,
            &format!("its card's base_model {one:?} is no <org>/<repo>"),
        )),
    }
}

/// The `license` of `card`, else of the checkpoint's card.
async fn license(hub: &Hub, card: Card, files: &Files) -> Result<Option<String>, RegistryError> {
    if card.license.is_some() {
        return Ok(card.license);
    }
    Ok(match files {
        Files::Hub(snapshot) => self::card(hub, snapshot).await?.license,
        Files::Dir(dir) => match std::fs::read_to_string(dir.join(CARD)) {
            Ok(text) => Card::parse(&text).license,
            Err(_) => None,
        },
    })
}

/// The card of `snapshot`, empty when it has none.
async fn card(hub: &Hub, snapshot: &Snapshot) -> Result<Card, RegistryError> {
    if !snapshot.has(CARD) {
        return Ok(Card::default());
    }
    let path = hub.file(snapshot, CARD).await?;
    let text = std::fs::read_to_string(&path).map_err(|err| RegistryError::Io { path, err })?;
    Ok(Card::parse(&text))
}

/// What the front matter of a model card says that a fine-tune reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Card {
    /// The repositories `base_model` names, `<org>/<repo>` each.
    pub base_models: Vec<String>,
    pub license: Option<String>,
}

impl Card {
    /// The card `text`: of the YAML between its first line, `---`, and the next `---` line, the top-level keys
    /// `base_model` and `license`, each a plain or quoted scalar, a flow list (`[a, b]`) or a block list (`- a` lines
    /// under the key).
    pub fn parse(text: &str) -> Card {
        let mut lines = text.lines();
        if lines.next().map(str::trim_end) != Some("---") {
            return Card::default();
        }
        let front: Vec<&str> = lines.take_while(|line| line.trim_end() != "---").collect();
        let values = |key: &str| -> Vec<String> {
            let Some((at, inline)) = front.iter().enumerate().find_map(|(at, line)| {
                let rest = line.strip_prefix(key)?.strip_prefix(':')?;
                Some((at, rest.trim()))
            }) else {
                return Vec::new();
            };
            let values: Vec<&str> = if inline.is_empty() {
                front[at + 1..]
                    .iter()
                    .map_while(|line| line.trim_start().strip_prefix('-'))
                    .collect()
            } else if let Some(list) = inline.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                list.split(',').collect()
            } else {
                vec![inline]
            };
            values
                .into_iter()
                .map(|value| {
                    let value = value.trim();
                    ['"', '\'']
                        .iter()
                        .find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q))
                        .unwrap_or(value)
                        .to_string()
                })
                .filter(|value| !value.is_empty())
                .collect()
        };
        Card {
            base_models: values("base_model"),
            license: values("license").into_iter().next(),
        }
    }
}

/// The refusal of a base whose checkpoint is unknown (`what`, `why`), naming `--checkpoint` (Q15, Q18).
fn unknown_checkpoint(what: String, why: &str) -> RegistryError {
    RegistryError::Invalid {
        what,
        msg: format!(
            "{why}; name the checkpoint to train from with --checkpoint {}<org>/<repo> or --checkpoint <dir>",
            refs::HF_PREFIX
        ),
    }
}

/// `hf.co/<org>/<repo>` as its parts; `what` names what it must be.
fn whole_repo(reference: &str, what: &str) -> Result<(String, String), RegistryError> {
    match Ref::parse(reference)? {
        Ref::Hf {
            org,
            repo,
            file: None,
        } => Ok((org, repo)),
        _ => Err(RegistryError::Invalid {
            what: reference.to_string(),
            msg: format!(
                "{what} is a whole repository, {}<org>/<repo>",
                refs::HF_PREFIX
            ),
        }),
    }
}

/// The file name of `path`.
fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Refuses the checkpoint `what` when it `holds` `rl_agent_config.json`: a THX-01 decision model, which reads no
/// prompt row and so trains on none (Q17).
fn undecided(what: &str, holds: bool) -> Result<(), RegistryError> {
    if holds {
        return Err(RegistryError::Invalid {
            what: what.to_string(),
            msg: format!(
                "it is a decision model (its listing holds {}), which reads no prompt rows, and ardana finetune \
                 trains causal models only",
                thx01::DECISION_CONFIG
            ),
        });
    }
    Ok(())
}

impl Base {
    /// The bytes of the checkpoint's safetensors, which its memory estimate reads before any of them downloads: those
    /// of a directory's files; for a Hub checkpoint, the sizes its listing gives the files its index names (else
    /// `model.safetensors`), offline those of the files in the hub cache.
    pub async fn weight_bytes(&self) -> Result<u64, RegistryError> {
        let snapshot = match &self.files {
            Files::Dir(dir) => {
                let files = ardana_core::snapshot::weight_files(dir).map_err(|msg| {
                    RegistryError::Invalid {
                        what: dir.display().to_string(),
                        msg,
                    }
                })?;
                let mut bytes = 0;
                for path in files {
                    bytes += std::fs::metadata(&path)
                        .map_err(|err| RegistryError::Io { path, err })?
                        .len();
                }
                return Ok(bytes);
            }
            Files::Hub(snapshot) => snapshot,
        };
        let hub = Hub::from_env(false)?;
        let files = safetensors(&hub, snapshot).await?;
        hub.sizes(snapshot, &files).await
    }

    /// The checkpoint's snapshot directory: a directory as it is; a Hub checkpoint downloaded at its commit into the hub
    /// cache (unless the cache holds it, or offline from the cache alone), `config.json`, its safetensors and every other
    /// file at the repository's root but weights of other formats.
    pub async fn download(&self, progress: bool) -> Result<PathBuf, RegistryError> {
        let snapshot = match &self.files {
            Files::Dir(dir) => return Ok(dir.clone()),
            Files::Hub(snapshot) => snapshot,
        };
        let hub = Hub::from_env(progress)?;
        let config = hub.file(snapshot, CONFIG).await?;
        let others = snapshot.files.iter().filter(|file| {
            !file.contains('/')
                && !file.starts_with('.')
                && !OTHER_WEIGHTS
                    .iter()
                    .any(|weights| file.to_ascii_lowercase().ends_with(weights))
        });
        for file in others {
            hub.file(snapshot, file).await?;
        }
        for file in safetensors(&hub, snapshot).await? {
            hub.file(snapshot, &file).await?;
        }
        config
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| RegistryError::Invalid {
                what: config.display().to_string(),
                msg: "a cache file outside a snapshot directory".into(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Q15: a card's `base_model` is read as Hub cards write it: a plain or quoted scalar, a flow list or a block list;
    /// a card without front matter names nothing.
    #[test]
    fn cards_name_their_base_models() {
        let qwen = "---\nlicense: apache-2.0\nlicense_link: https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/blob/main/LICENSE\n\
                    pipeline_tag: text-generation\nbase_model: Qwen/Qwen3-0.6B\n---\n\n# Qwen3-0.6B-GGUF\nbase_model: no\n";
        assert_eq!(
            Card::parse(qwen),
            Card {
                base_models: vec!["Qwen/Qwen3-0.6B".into()],
                license: Some("apache-2.0".into()),
            }
        );
        let spark = "---\nlicense: apache-2.0\ntags:\n- llm\nbase_model:\n- XHToken/Spark-X2.5-1.7B-Base\n---\n";
        assert_eq!(
            Card::parse(spark).base_models,
            ["XHToken/Spark-X2.5-1.7B-Base"]
        );
        let merge = "---\nbase_model:\n  - 'a/one'\n  - \"b/two\"\nlicense: \"mit\"\n---\n";
        assert_eq!(
            Card::parse(merge),
            Card {
                base_models: vec!["a/one".into(), "b/two".into()],
                license: Some("mit".into()),
            }
        );
        assert_eq!(
            Card::parse("---\r\nbase_model: [a/one, 'b/two']\r\n---\r\n").base_models,
            ["a/one", "b/two"]
        );
        assert_eq!(
            Card::parse("# A card\nbase_model: a/one\n"),
            Card::default()
        );
    }

    /// Q14: an `auto_map` in `config.json` or in `tokenizer_config.json` runs the checkpoint's own code (the second
    /// through llama.cpp's converter, which loads the tokenizer with `trust_remote_code=True`), so either is refused
    /// without `--trust-remote-code`, naming the file and the code; neither, or no `tokenizer_config.json`, is not.
    #[test]
    fn auto_map_in_either_config_runs_remote_code() {
        let auto_map =
            serde_json::json!({"auto_map": {"AutoTokenizer": ["tokenization_x.XTokenizer", null]}});
        let plain = serde_json::json!({"architectures": ["Qwen3ForCausalLM"]});
        let code = ["tokenization_x.py".to_string()];
        assert_eq!(remote_code(&plain, None, &code), None);
        assert_eq!(remote_code(&plain, Some(&plain), &code), None);
        assert_eq!(
            remote_code(&auto_map, None, &code).as_deref(),
            Some(
                "its config.json has auto_map, so transformers loads it by running the Python code it names \
                 (tokenization_x.py); pass --trust-remote-code to run that code"
            )
        );
        assert_eq!(
            remote_code(&plain, Some(&auto_map), &[]).as_deref(),
            Some(
                "its tokenizer_config.json has auto_map, so transformers loads its tokenizer by running the Python \
                 code it names (code from other repositories); pass --trust-remote-code to run that code"
            )
        );
    }
}
