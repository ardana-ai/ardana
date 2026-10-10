//! Model references, the model library, `models.toml`, and Hugging Face and Ollama resolution.
//!
//! `ardana pull <ref>` resolves a library name ([`library`], read at run time from the published library document)
//! or a reference ([`refs::Ref`]) to weights and a tokenizer in place (the hub cache, the Ollama store or a local
//! file; nothing is copied), derives the model's profile (from `decider_config.json` when the weights come with one,
//! else [`ModelProfile::stock`] in the chat layout) and records the result as one entry of `$ARDANA_HOME/models.toml`
//! ([`Registry`]). The server and `ardana run <name>` look names up with [`Registry::named`]: a pulled entry, else a
//! library model they pull first. [`pull_browser`] resolves a library model's browser variant (ONNX weights a
//! visitor's tab runs) the same way, into the hub cache only: a browser variant is never a registry entry.

pub mod hub;
pub mod library;
pub mod ollama;
pub mod refs;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ardana_core::{
    ChatTemplateError, Layout, LoadOptions, LoadedModel, ModelProfile, ProfileError, Runtimes,
    chat_layout, from_decider_config, read_template,
};
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;

use crate::hub::{Hub, Snapshot};
use crate::library::{BrowserEntry, Library, LibraryPick, MODELS_PAGE};
use crate::refs::{DEFAULT_QUANT, HfFile, Ref, RefError, companion_of, matches_quant};

pub use ardana_core::LayoutKind;

/// The registry file inside `$ARDANA_HOME`.
pub const MODELS_FILE: &str = "models.toml";
/// The variable naming the Ardana home; without it the home is `~/.ardana`.
pub const HOME_VAR: &str = "ARDANA_HOME";
/// The decider config a decider-format model ships next to its weights.
pub const DECIDER_CONFIG: &str = "decider_config.json";
/// The Hugging Face tokenizer every model needs.
pub const TOKENIZER_FILE: &str = "tokenizer.json";
/// The tokenizer files `ardana pull` fetches with `tokenizer.json` when the repository has them: the special tokens
/// and the chat template the chat layout reads.
const TOKENIZER_EXTRAS: [&str; 2] = [
    ardana_core::chat::CONFIG_FILE,
    ardana_core::chat::TEMPLATE_FILE,
];
/// The files a tab downloads to run a browser variant: the graph, its external weights and the tokenizer.
pub const BROWSER_FILES: [&str; 3] = ["model.onnx", "model.onnx.data", TOKENIZER_FILE];

/// One registry entry: a named model resolved to its files, runtime and profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedModel {
    pub name: String,
    /// The reference it was pulled from (a local path is recorded absolute).
    pub source: String,
    /// The weights, read in place: a hub cache snapshot path, an Ollama blob or a local file, or the hub cache
    /// snapshot directory of a safetensors checkpoint.
    pub weights: PathBuf,
    /// The Hugging Face `tokenizer.json`; the chat template is read next to it.
    pub tokenizer: PathBuf,
    /// The id of the runtime that supported the weights at pull time.
    pub runtime: String,
    pub profile: ModelProfile,
    /// The UTC date of the pull, `YYYY-MM-DD`.
    pub pulled_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Ref(#[from] RefError),
    #[error("{}", unknown_model(.name, .available))]
    UnknownModel {
        name: String,
        available: Vec<String>,
    },
    /// A library family without the tag asked for (Q11, Q12): its canonical tags, in document order.
    #[error("{family} has no tag {tag:?}; its tags are {}", .tags.join(", "))]
    UnknownTag {
        family: String,
        tag: String,
        tags: Vec<String>,
    },
    #[error("{what}: {msg}")]
    Invalid { what: String, msg: String },
    #[error("{}: {err}", .path.display())]
    Io { path: PathBuf, err: std::io::Error },
    #[error("{repo}: {err}")]
    Hub { repo: String, err: hf_hub::HFError },
    #[error("reading the library at {url}: {msg}")]
    Library { url: String, msg: String },
    #[error(
        "{what} is not in the Hugging Face cache at {} and {} is set; unset it to download",
        .cache.display(),
        hub::OFFLINE_VAR
    )]
    NotCached { what: String, cache: PathBuf },
    #[error("loading the tokenizer {}: {msg}", .path.display())]
    Tokenizer { path: PathBuf, msg: String },
    #[error("reading {}: {err}", .path.display())]
    Profile { path: PathBuf, err: ProfileError },
    #[error("reading the chat template in {}: {err}", .dir.display())]
    ChatTemplate {
        dir: PathBuf,
        err: ChatTemplateError,
    },
    #[error("model {name} ({reference}): {} is missing; pull it again", .path.display())]
    Missing {
        name: String,
        reference: String,
        path: PathBuf,
    },
    /// A checkpoint whose `config.json#architectures` no runtime implements (Q7).
    #[error(
        "{reference}: no runtime implements {} ({}#architectures)",
        named_architectures(.architectures),
        ardana_core::snapshot::CONFIG
    )]
    Architecture {
        reference: String,
        architectures: Vec<String>,
    },
    #[error("no runtime can load {} ({reference}){}", .weights.display(), hint(.reference))]
    NoRuntime { reference: String, weights: PathBuf },
    #[error("loading model {name} ({reference}): {err:#}{}", hint(.reference))]
    Load {
        name: String,
        reference: String,
        weights: PathBuf,
        err: anyhow::Error,
    },
}

fn unknown_model(name: &str, available: &[String]) -> String {
    let pulled = if available.is_empty() {
        "none pulled yet".to_string()
    } else {
        format!("pulled: {}", available.join(", "))
    };
    format!(
        "no model named {name:?}; {pulled}; the library at {MODELS_PAGE} is pulled on first use"
    )
}

/// The architectures a `config.json` names, for [`RegistryError::Architecture`].
fn named_architectures(architectures: &[String]) -> String {
    if architectures.is_empty() {
        "a checkpoint that names no architecture".to_string()
    } else {
        architectures.join(", ")
    }
}

/// What to do instead when weights from `source` do not load: Ollama converts some models its own way, and upstream
/// llama.cpp cannot load those GGUFs.
fn hint(source: &str) -> String {
    if source.starts_with(refs::OLLAMA_PREFIX) {
        format!(
            "; Ollama's own GGUF conversions of some models do not load in upstream llama.cpp, so pull the model from \
             a Hugging Face GGUF repository instead: `ardana pull {}<org>/<repo>-GGUF:{DEFAULT_QUANT} --tokenizer \
             {}<org>/<repo>`",
            refs::HF_PREFIX,
            refs::HF_PREFIX
        )
    } else {
        String::new()
    }
}

impl ResolvedModel {
    /// The weight files: the weights file itself, or the safetensors files a snapshot directory's index names (else
    /// its `model.safetensors`, [`ardana_core::snapshot::weight_files`]).
    pub fn weight_files(&self) -> Result<Vec<PathBuf>, String> {
        if self.weights.is_dir() {
            ardana_core::snapshot::weight_files(&self.weights)
        } else {
            Ok(vec![self.weights.clone()])
        }
    }

    /// The bytes of the [`ResolvedModel::weight_files`] together.
    pub fn weights_bytes(&self) -> std::io::Result<u64> {
        let files = self.weight_files().map_err(std::io::Error::other)?;
        files
            .iter()
            .map(|file| std::fs::metadata(file).map(|meta| meta.len()))
            .sum()
    }

    /// Loads the weights with the first runtime that supports them ([`Runtimes::for_weights`]).
    pub fn load(
        &self,
        runtimes: &Runtimes,
        opts: &LoadOptions,
    ) -> Result<Box<dyn LoadedModel>, RegistryError> {
        let runtime =
            runtimes
                .for_weights(&self.weights)
                .ok_or_else(|| RegistryError::NoRuntime {
                    reference: self.source.clone(),
                    weights: self.weights.clone(),
                })?;
        runtime
            .load(&self.weights, opts)
            .map_err(|err| RegistryError::Load {
                name: self.name.clone(),
                reference: self.source.clone(),
                weights: self.weights.clone(),
                err,
            })
    }
}

/// `models.toml`: `[[model]]` tables in registry order.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelsFile {
    #[serde(default, rename = "model")]
    models: Vec<ResolvedModel>,
}

/// The named models of one Ardana home.
#[derive(Debug, Clone)]
pub struct Registry {
    path: PathBuf,
    models: Vec<ResolvedModel>,
}

impl Registry {
    /// `$ARDANA_HOME`, else `~/.ardana`.
    pub fn home() -> Result<PathBuf, RegistryError> {
        if let Some(home) = std::env::var_os(HOME_VAR) {
            return Ok(PathBuf::from(home));
        }
        let home = std::env::home_dir().ok_or_else(|| RegistryError::Invalid {
            what: "the Ardana home".into(),
            msg: format!("{HOME_VAR} is not set and the home directory is unknown"),
        })?;
        Ok(home.join(".ardana"))
    }

    /// The registry of [`Registry::home`].
    pub fn open_default() -> Result<Registry, RegistryError> {
        Registry::open(&Registry::home()?)
    }

    /// The registry in `home/models.toml`; empty when the file does not exist yet.
    pub fn open(home: &Path) -> Result<Registry, RegistryError> {
        let path = home.join(MODELS_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Registry {
                    path,
                    models: Vec::new(),
                });
            }
            Err(err) => return Err(RegistryError::Io { path, err }),
        };
        let invalid = |msg: String| RegistryError::Invalid {
            what: path.display().to_string(),
            msg,
        };
        let file: ModelsFile = toml::from_str(&text).map_err(|err| invalid(err.to_string()))?;
        for (i, model) in file.models.iter().enumerate() {
            if file.models[..i].iter().any(|m| m.name == model.name) {
                return Err(invalid(format!(
                    "the model name {:?} appears twice",
                    model.name
                )));
            }
        }
        Ok(Registry {
            path,
            models: file.models,
        })
    }

    /// The `models.toml` this registry reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The model names in file order.
    pub fn names(&self) -> Vec<String> {
        self.models.iter().map(|m| m.name.clone()).collect()
    }

    /// Every entry in file order.
    pub fn entries(&self) -> &[ResolvedModel] {
        &self.models
    }

    /// The entry named `name`, as recorded.
    pub fn entry(&self, name: &str) -> Result<&ResolvedModel, RegistryError> {
        self.models
            .iter()
            .find(|m| m.name == name)
            .ok_or_else(|| RegistryError::UnknownModel {
                name: name.to_string(),
                available: self.names(),
            })
    }

    /// What `name` means here (Q25): the entry of that name, else the quant `library` names by it (which may be
    /// pulled under its canonical name, `decider:2b` for `decider` or `decider:2b-q4_k_m`, Q10), else an
    /// [`RegistryError::UnknownModel`] listing the pulled names; a tag the library's family lacks is its
    /// [`RegistryError::UnknownTag`].
    pub fn named<'a>(&'a self, name: &str, library: &Library) -> Result<Named<'a>, RegistryError> {
        if let Some(model) = self.models.iter().find(|m| m.name == name) {
            return Ok(Named::Pulled(model));
        }
        let Some(pick) = library.find(name)? else {
            return Err(RegistryError::UnknownModel {
                name: name.to_string(),
                available: self.names(),
            });
        };
        let canonical = pick.name();
        Ok(match self.models.iter().find(|m| m.name == canonical) {
            Some(model) => Named::Pulled(model),
            None => Named::Library(Box::new(pick)),
        })
    }

    /// When `models.toml` was last written; `None` while it does not exist.
    pub fn modified(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok()
    }

    /// The entry named `name`, with its weight files and tokenizer still in place.
    pub fn resolve(&self, name: &str) -> Result<ResolvedModel, RegistryError> {
        let model = self.entry(name)?;
        let missing = |path: &Path| RegistryError::Missing {
            name: model.name.clone(),
            reference: model.source.clone(),
            path: path.to_path_buf(),
        };
        let mut files = model.weight_files().map_err(|_| missing(&model.weights))?;
        files.push(model.tokenizer.clone());
        match files.iter().find(|path| !path.is_file()) {
            Some(path) => Err(missing(path)),
            None => Ok(model.clone()),
        }
    }

    /// Adds `model`, replacing an entry of the same name in place.
    pub fn insert(&mut self, model: ResolvedModel) {
        match self.models.iter_mut().find(|m| m.name == model.name) {
            Some(slot) => *slot = model,
            None => self.models.push(model),
        }
    }

    /// Removes the entry named `name`; the files it points at stay where they are.
    pub fn remove(&mut self, name: &str) -> Result<ResolvedModel, RegistryError> {
        let index = self
            .models
            .iter()
            .position(|m| m.name == name)
            .ok_or_else(|| RegistryError::UnknownModel {
                name: name.to_string(),
                available: self.names(),
            })?;
        Ok(self.models.remove(index))
    }

    /// Writes `models.toml` (through a temporary file renamed over it).
    pub fn save(&self) -> Result<(), RegistryError> {
        let file = ModelsFile {
            models: self.models.clone(),
        };
        let text = toml::to_string(&file).map_err(|err| RegistryError::Invalid {
            what: self.path.display().to_string(),
            msg: err.to_string(),
        })?;
        write_atomically(&self.path, text.as_bytes())
    }
}

/// Writes `bytes` to `path` through a temporary file beside it (`<path>.tmp`) renamed over it, creating the
/// directory, so a reader never sees a partial file: `models.toml` and the library's cache are written this way.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |err| RegistryError::Io { path, err }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io(path))
}

/// What a model name means in a registry ([`Registry::named`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Named<'a> {
    /// A pulled entry.
    Pulled(&'a ResolvedModel),
    /// A library model not pulled yet; [`pull_library`] it. Boxed: a size outweighs a reference many times.
    Library(Box<LibraryPick>),
}

/// What `ardana pull` takes besides the reference; each field overrides what the reference implies.
#[derive(Debug, Clone, Default)]
pub struct PullOptions {
    /// The registry name (default [`Ref::default_name`]).
    pub name: Option<String>,
    /// Where the tokenizer comes from: `hf.co/<org>/<repo>` or a local `tokenizer.json` (or its directory); default
    /// the weights' own repository or directory.
    pub tokenizer: Option<String>,
    /// The layout (default plain with `decider_config.json`, else chat).
    pub layout: Option<LayoutKind>,
    /// Print each download's progress to stderr (bytes so far and the file's size).
    pub progress: bool,
}

/// Resolves `reference`, a name `library` holds or a reference, to a registry entry, downloading Hugging Face files
/// into the hub cache unless `HF_HUB_OFFLINE` is set: a library name is pulled by [`pull_library`], anything else
/// as the reference it is. Nothing is written to the registry; the caller [`Registry::insert`]s the result.
pub async fn pull(
    reference: &str,
    opts: &PullOptions,
    runtimes: &Runtimes,
    library: &Library,
) -> Result<ResolvedModel, RegistryError> {
    match library.find(reference)? {
        Some(pick) => pull_library(&pick, opts, runtimes).await,
        None => pull_ref(reference, opts, runtimes, Revisions::default()).await,
    }
}

/// Pulls the library quant `pick`: its file, `hf.co/<org>/<repo>:<file>` (Q14), under its canonical
/// [`LibraryPick::name`], with the size's tokenizer, layout and release date where `opts` and the weights give none.
/// The weights are read at `gguf.commit` and the size's tokenizer at its commit, never at `main`; a `--tokenizer` of
/// `opts` is read as the reference it is.
pub async fn pull_library(
    pick: &LibraryPick,
    opts: &PullOptions,
    runtimes: &Runtimes,
) -> Result<ResolvedModel, RegistryError> {
    let tokenizer = pick.size.tokenizer.as_ref();
    let revisions = Revisions {
        weights: Some(&pick.size.gguf.commit),
        tokenizer: match opts.tokenizer {
            Some(_) => None,
            None => tokenizer.map(|pin| pin.commit.as_str()),
        },
    };
    let opts = PullOptions {
        name: Some(opts.name.clone().unwrap_or_else(|| pick.name())),
        tokenizer: opts
            .tokenizer
            .clone()
            .or_else(|| tokenizer.map(|pin| pin.repo.clone())),
        layout: opts.layout.or(pick.size.layout),
        progress: opts.progress,
    };
    let mut model = pull_ref(&pick.reference(), &opts, runtimes, revisions).await?;
    model
        .profile
        .release_date
        .get_or_insert_with(|| pick.size.release_date.clone());
    Ok(model)
}

/// The commits a pull reads its Hugging Face repositories at: a library size's pins; `None` reads `main`, as an
/// `hf.co/` reference does.
#[derive(Debug, Clone, Copy, Default)]
struct Revisions<'a> {
    /// The weights repository's commit.
    weights: Option<&'a str>,
    /// The commit of the tokenizer repository [`PullOptions::tokenizer`] names.
    tokenizer: Option<&'a str>,
}

async fn pull_ref(
    reference: &str,
    opts: &PullOptions,
    runtimes: &Runtimes,
    revisions: Revisions<'_>,
) -> Result<ResolvedModel, RegistryError> {
    let parsed = Ref::parse(reference)?;
    let name = match &opts.name {
        Some(name) => name.clone(),
        None => parsed.default_name(),
    };
    if name.is_empty() || name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(RegistryError::Invalid {
            what: format!("the model name {name:?}"),
            msg: "a name is non-empty and has no whitespace".into(),
        });
    }
    let hub = Hub::from_env(opts.progress)?;
    let mut source = reference.to_string();
    let (weights, config, own_tokenizer) = match &parsed {
        Ref::Hf { org, repo, file } => {
            let snapshot = hub.snapshot(org, repo, revisions.weights).await?;
            let weights = if file.is_none() && is_checkpoint(&snapshot) {
                snapshot_weights(&hub, &snapshot, runtimes).await?
            } else {
                let gguf = pick_gguf(&snapshot, file.as_ref())?;
                hub.file(&snapshot, &gguf).await?
            };
            let config = if snapshot.has(DECIDER_CONFIG) {
                Some(hub.file(&snapshot, DECIDER_CONFIG).await?)
            } else {
                None
            };
            let tokenizer = match (&opts.tokenizer, snapshot.has(TOKENIZER_FILE)) {
                (None, true) => Some(hf_tokenizer(&hub, &snapshot).await?),
                _ => None,
            };
            (weights, config, tokenizer)
        }
        Ref::Ollama {
            namespace,
            name,
            tag,
        } => (ollama::blob(reference, namespace, name, tag)?, None, None),
        Ref::Local(path) => {
            let weights = local_file(path)?;
            source = weights.display().to_string();
            let beside = |file: &str| {
                let path = weights.with_file_name(file);
                path.is_file().then_some(path)
            };
            (
                weights.clone(),
                beside(DECIDER_CONFIG),
                beside(TOKENIZER_FILE),
            )
        }
    };
    let tokenizer_path = match (&opts.tokenizer, own_tokenizer) {
        (Some(tokenizer), _) => tokenizer_ref(&hub, tokenizer, revisions.tokenizer).await?,
        (None, Some(path)) => path,
        (None, None) => {
            return Err(RegistryError::Invalid {
                what: source,
                msg: format!(
                    "no {TOKENIZER_FILE} comes with these weights; pass --tokenizer {}<org>/<repo> naming the \
                     repository the weights were converted from",
                    refs::HF_PREFIX
                ),
            });
        }
    };
    let runtime = runtimes
        .for_weights(&weights)
        .ok_or_else(|| RegistryError::NoRuntime {
            reference: source.clone(),
            weights: weights.clone(),
        })?
        .id()
        .to_string();
    let tokenizer = load_tokenizer(&tokenizer_path)?;
    let layout = opts
        .layout
        .unwrap_or_else(|| default_layout(config.as_deref()));
    let profile = read_profile(
        &tokenizer_path,
        &tokenizer,
        config.as_deref(),
        layout,
        &name,
    )?;
    Ok(ResolvedModel {
        name,
        source,
        weights,
        tokenizer: tokenizer_path,
        runtime,
        profile,
        pulled_at: today(),
    })
}

/// The layout of a model that names none: plain with a `decider_config.json`, else chat.
fn default_layout(config: Option<&Path>) -> LayoutKind {
    match config {
        Some(_) => LayoutKind::Plain,
        None => LayoutKind::Chat,
    }
}

/// A library size's browser variant in the hub cache: the files a tab downloads ([`BROWSER_FILES`]) and the profile it
/// reads them with.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserModel {
    /// The size's canonical name.
    pub name: String,
    /// `hf.co/<org>/<repo>`, the ONNX repository.
    pub reference: String,
    /// The snapshot commit the files come from.
    pub commit: String,
    /// The cache path (`snapshots/<commit>/<file>`) of each of [`BROWSER_FILES`], in that order.
    pub files: Vec<PathBuf>,
    /// The profile `ardana pull <name>` derives for the model, from the same tokenizer and config files.
    pub profile: ModelProfile,
}

impl BrowserModel {
    /// The cache path of `file`, one of [`BROWSER_FILES`].
    pub fn file(&self, file: &str) -> Option<&Path> {
        let index = BROWSER_FILES.iter().position(|f| *f == file)?;
        self.files.get(index).map(PathBuf::as_path)
    }
}

/// Resolves the browser variant of the library size `model` in the hub cache, cache first: a variant the cache holds
/// completely is read in place without a Hub call (as `HF_HUB_OFFLINE` reads a repository), so it is pulled once;
/// otherwise its repository is pulled, unless `HF_HUB_OFFLINE` is set. Either way the files are those of the size's
/// `browser.commit`, never of `main`. The profile is derived as [`pull`] derives the size's own: `decider_config.json`
/// beside the files, else the stock profile named after the canonical name in the size's layout (Q17), dated by the
/// size's release date.
pub async fn pull_browser(
    model: &LibraryPick,
    progress: bool,
) -> Result<BrowserModel, RegistryError> {
    let browser = browser_of(model)?;
    browser_variant(&Hub::from_env(progress)?, model, browser).await
}

/// The browser variant of `model`; an error when it has none.
fn browser_of(model: &LibraryPick) -> Result<&BrowserEntry, RegistryError> {
    model
        .size
        .browser
        .as_ref()
        .ok_or_else(|| RegistryError::Invalid {
            what: format!("the model {:?}", model.name()),
            msg: "the library model has no browser variant".into(),
        })
}

/// The browser variant `browser` of `model` through `hub`, at its commit, cache first: the cache's copy when it holds
/// all of it, else (whatever the cache lacks or holds unreadable) the repository from the Hub, unless `hub` is offline.
async fn browser_variant(
    hub: &Hub,
    model: &LibraryPick,
    browser: &BrowserEntry,
) -> Result<BrowserModel, RegistryError> {
    match browser_files(&hub.cache_only(), model, browser).await {
        Err(_) if !hub.is_offline() => browser_files(hub, model, browser).await,
        cached => cached,
    }
}

async fn browser_files(
    hub: &Hub,
    model: &LibraryPick,
    browser: &BrowserEntry,
) -> Result<BrowserModel, RegistryError> {
    let located = browser_paths(hub, model, browser).await?;
    let tokenizer = load_tokenizer(&located.tokenizer)?;
    let layout = model
        .size
        .layout
        .unwrap_or_else(|| default_layout(located.config.as_deref()));
    let name = model.name();
    let mut profile = read_profile(
        &located.tokenizer,
        &tokenizer,
        located.config.as_deref(),
        layout,
        &name,
    )?;
    profile
        .release_date
        .get_or_insert_with(|| model.size.release_date.clone());
    Ok(BrowserModel {
        name,
        reference: located.snapshot.id(),
        commit: located.snapshot.commit().to_string(),
        files: located.files,
        profile,
    })
}

/// Where the files of a browser variant are in the hub cache: every file its pull reads.
struct BrowserPaths {
    snapshot: Snapshot,
    tokenizer: PathBuf,
    /// `decider_config.json`, for a model whose profile it gives.
    config: Option<PathBuf>,
    /// [`BROWSER_FILES`], in that order.
    files: Vec<PathBuf>,
}

/// The cache paths of every file of the browser variant `browser` of `model`, at its commit, through `hub`, which
/// downloads what the cache lacks unless it is offline.
async fn browser_paths(
    hub: &Hub,
    model: &LibraryPick,
    browser: &BrowserEntry,
) -> Result<BrowserPaths, RegistryError> {
    let Ref::Hf {
        org,
        repo,
        file: None,
    } = Ref::parse(&browser.repo)?
    else {
        return Err(RegistryError::Invalid {
            what: browser.repo.clone(),
            msg: format!(
                "a browser variant is a whole repository, {}<org>/<repo>",
                refs::HF_PREFIX
            ),
        });
    };
    let snapshot = hub.snapshot(&org, &repo, Some(&browser.commit)).await?;
    let tokenizer = hf_tokenizer(hub, &snapshot).await?;
    // A size whose family names no layout reads its profile from `decider_config.json`. A cached snapshot lists only
    // the files the cache holds, so there the config is required: a cache without it holds part of the variant, which
    // would read as the stock profile in the chat layout.
    let config =
        if snapshot.has(DECIDER_CONFIG) || (hub.is_offline() && model.size.layout.is_none()) {
            Some(hub.file(&snapshot, DECIDER_CONFIG).await?)
        } else {
            None
        };
    let mut files = Vec::with_capacity(BROWSER_FILES.len());
    for file in BROWSER_FILES {
        files.push(if file == TOKENIZER_FILE {
            tokenizer.clone()
        } else {
            hub.file(&snapshot, file).await?
        });
    }
    Ok(BrowserPaths {
        snapshot,
        tokenizer,
        config,
        files,
    })
}

/// The hub cache, asked whether it holds browser variants whole: built once, its lookups read the cache alone and
/// never the Hub.
pub struct BrowserCache(Hub);

impl BrowserCache {
    /// The hub cache the environment names, as a pull finds it (`HF_HOME` and the other variables, [`Hub::from_env`]).
    pub fn from_env() -> Result<BrowserCache, RegistryError> {
        Ok(BrowserCache(Hub::from_env(false)?.cache_only()))
    }

    /// The hub cache at `dir` (a `hub/` directory: `models--<org>--<repo>/...`), whatever the environment names.
    pub fn at(dir: &Path) -> Result<BrowserCache, RegistryError> {
        let client = hf_hub::HFClient::builder()
            .cache_dir(dir)
            .build()
            .map_err(|err| RegistryError::Hub {
                repo: "the Hugging Face client".into(),
                err,
            })?;
        Ok(BrowserCache(Hub::new(client, true, false)))
    }

    /// The browser variant of the library size `model` as the cache holds it, read as [`pull_browser`] reads it from
    /// there: its commit and the profile; an error when the cache does not hold every file of it at the size's commit.
    pub async fn variant(&self, model: &LibraryPick) -> Result<BrowserModel, RegistryError> {
        let browser = browser_of(model)?;
        browser_files(&self.0, model, browser).await
    }

    /// Whether the cache holds the browser variant of the library size `model` whole at the size's commit: every file
    /// [`pull_browser`] reads, so a pull reads it in place, without the Hub. The files are found, not read: one that
    /// does not read sends a pull to the Hub all the same.
    pub async fn holds(&self, model: &LibraryPick) -> bool {
        let Some(browser) = &model.size.browser else {
            return false;
        };
        browser_paths(&self.0, model, browser).await.is_ok()
    }
}

/// Whether `snapshot` is a safetensors checkpoint rather than a GGUF repository: it holds no GGUF and has a
/// `config.json` (Q6).
fn is_checkpoint(snapshot: &Snapshot) -> bool {
    snapshot.has(ardana_core::snapshot::CONFIG) && !snapshot.files.iter().any(|f| is_gguf(f))
}

fn is_gguf(file: &str) -> bool {
    file.to_ascii_lowercase().ends_with(".gguf")
}

/// The snapshot directory of the checkpoint `snapshot`, which carries its commit as a GGUF's path does (Q4): its
/// `config.json`, then, once a runtime implements an architecture it names (Q7), the safetensors files its index
/// names (else its one `model.safetensors`).
async fn snapshot_weights(
    hub: &Hub,
    snapshot: &Snapshot,
    runtimes: &Runtimes,
) -> Result<PathBuf, RegistryError> {
    use ardana_core::snapshot::{CONFIG, INDEX, SINGLE, architectures};
    let config = hub.file(snapshot, CONFIG).await?;
    let dir = config
        .parent()
        .ok_or_else(|| RegistryError::Invalid {
            what: config.display().to_string(),
            msg: "a cache file outside a snapshot directory".into(),
        })?
        .to_path_buf();
    if runtimes.for_weights(&dir).is_none() {
        return Err(RegistryError::Architecture {
            reference: snapshot.id(),
            architectures: architectures(&read_json(&config)?),
        });
    }
    let files = if snapshot.has(INDEX) {
        let index = hub.file(snapshot, INDEX).await?;
        ardana_core::snapshot::shards(&read_json(&index)?).map_err(|msg| {
            RegistryError::Invalid {
                what: format!("{} file {INDEX}", snapshot.id()),
                msg,
            }
        })?
    } else if snapshot.has(SINGLE) {
        vec![SINGLE.to_string()]
    } else {
        return Err(RegistryError::Invalid {
            what: snapshot.id(),
            msg: format!("it has {CONFIG} but neither {INDEX} nor {SINGLE}"),
        });
    };
    for file in files {
        hub.file(snapshot, &file).await?;
    }
    Ok(dir)
}

/// The JSON document at `path`.
fn read_json(path: &Path) -> Result<serde_json::Value, RegistryError> {
    let text = std::fs::read_to_string(path).map_err(|err| RegistryError::Io {
        path: path.to_path_buf(),
        err,
    })?;
    serde_json::from_str(&text).map_err(|err| RegistryError::Invalid {
        what: path.display().to_string(),
        msg: err.to_string(),
    })
}

/// The GGUF of `snapshot` that `file` names; without `file`, the [`DEFAULT_QUANT`] one.
fn pick_gguf(snapshot: &Snapshot, file: Option<&HfFile>) -> Result<String, RegistryError> {
    let ggufs: Vec<&String> = snapshot.files.iter().filter(|f| is_gguf(f)).collect();
    let invalid = |msg: String| RegistryError::Invalid {
        what: snapshot.id(),
        msg,
    };
    let listing = || {
        if ggufs.is_empty() && file.is_none() {
            format!(
                "it has no GGUF files and no {}",
                ardana_core::snapshot::CONFIG
            )
        } else if ggufs.is_empty() {
            "it has no GGUF files".to_string()
        } else {
            let names: Vec<&str> = ggufs.iter().map(|f| f.as_str()).collect();
            format!("its GGUF files are {}", names.join(", "))
        }
    };
    let quant = match file {
        Some(HfFile::Name(name)) => {
            return if snapshot.has(name) {
                Ok(name.clone())
            } else {
                Err(invalid(format!("no file {name}; {}", listing())))
            };
        }
        Some(HfFile::Quant(quant)) => quant.as_str(),
        None => DEFAULT_QUANT,
    };
    let of_quant: Vec<&String> = ggufs
        .iter()
        .copied()
        .filter(|f| matches_quant(f, quant))
        .collect();
    // The model's file, not the projector or draft heads published next to it under its name.
    let matches: Vec<&String> = of_quant
        .iter()
        .copied()
        .filter(|f| !of_quant.iter().any(|model| companion_of(f, model)))
        .collect();
    match matches[..] {
        [one] => Ok(one.clone()),
        [] => Err(invalid(format!(
            "no GGUF for the quant {quant}; {}",
            listing()
        ))),
        _ => {
            let names: Vec<&str> = matches.iter().map(|f| f.as_str()).collect();
            Err(invalid(format!(
                "the quant {quant} matches {}; name one file as {}:<file>.gguf",
                names.join(", "),
                snapshot.id()
            )))
        }
    }
}

/// `tokenizer.json` of `snapshot`, fetched with the [`TOKENIZER_EXTRAS`] the repository has.
async fn hf_tokenizer(hub: &Hub, snapshot: &Snapshot) -> Result<PathBuf, RegistryError> {
    for extra in TOKENIZER_EXTRAS {
        if snapshot.has(extra) {
            hub.file(snapshot, extra).await?;
        }
    }
    hub.file(snapshot, TOKENIZER_FILE).await
}

/// The `tokenizer.json` a `--tokenizer` reference names: an `hf.co/<org>/<repo>` repository (at `commit`, else at
/// `main`), or a local file or directory.
async fn tokenizer_ref(
    hub: &Hub,
    reference: &str,
    commit: Option<&str>,
) -> Result<PathBuf, RegistryError> {
    let invalid = |msg: String| RegistryError::Invalid {
        what: format!("the tokenizer {reference}"),
        msg,
    };
    match Ref::parse(reference)? {
        Ref::Hf {
            org,
            repo,
            file: None,
        } => {
            let snapshot = hub.snapshot(&org, &repo, commit).await?;
            if !snapshot.has(TOKENIZER_FILE) {
                return Err(invalid(format!("the repository has no {TOKENIZER_FILE}")));
            }
            hf_tokenizer(hub, &snapshot).await
        }
        Ref::Hf { file: Some(_), .. } => Err(invalid(format!(
            "a tokenizer is a whole repository, {}<org>/<repo>, without a quant or file",
            refs::HF_PREFIX
        ))),
        Ref::Ollama { .. } => Err(invalid(format!(
            "Ollama models carry no {TOKENIZER_FILE}; name the Hugging Face repository the model comes from"
        ))),
        Ref::Local(path) => {
            let path = if path.is_dir() {
                path.join(TOKENIZER_FILE)
            } else {
                path
            };
            local_file(&path)
        }
    }
}

/// `path` made absolute without resolving symlinks, so a hub snapshot path keeps its snapshot directory (where the
/// files beside it live); it must be a file.
fn local_file(path: &Path) -> Result<PathBuf, RegistryError> {
    let io = |err| RegistryError::Io {
        path: path.to_path_buf(),
        err,
    };
    let absolute = std::path::absolute(path).map_err(io)?;
    let meta = std::fs::metadata(&absolute).map_err(io)?;
    if !meta.is_file() {
        return Err(RegistryError::Invalid {
            what: absolute.display().to_string(),
            msg: "not a file".into(),
        });
    }
    Ok(absolute)
}

/// Loads a Hugging Face `tokenizer.json`.
pub fn load_tokenizer(path: &Path) -> Result<Tokenizer, RegistryError> {
    Tokenizer::from_file(path).map_err(|err| RegistryError::Tokenizer {
        path: path.to_path_buf(),
        msg: err.to_string(),
    })
}

/// The profile of a model: `decider_config.json` when given, else [`ModelProfile::stock`] named `stock_name`; in
/// the chat layout, head and tail come from the chat template next to `tokenizer_path`.
pub fn read_profile(
    tokenizer_path: &Path,
    tokenizer: &Tokenizer,
    config: Option<&Path>,
    layout: LayoutKind,
    stock_name: &str,
) -> Result<ModelProfile, RegistryError> {
    let layout = match layout {
        LayoutKind::Plain => Layout::Plain,
        LayoutKind::Chat => {
            let dir = tokenizer_path.parent().unwrap_or(Path::new("."));
            let chat_err = |err| RegistryError::ChatTemplate {
                dir: dir.to_path_buf(),
                err,
            };
            let (template, specials) = read_template(dir).map_err(chat_err)?;
            chat_layout(&template, tokenizer, &specials).map_err(chat_err)?
        }
    };
    let Some(path) = config else {
        return Ok(ModelProfile::stock(stock_name, layout));
    };
    let invalid = |msg: String| RegistryError::Invalid {
        what: path.display().to_string(),
        msg,
    };
    let text = std::fs::read_to_string(path).map_err(|err| RegistryError::Io {
        path: path.to_path_buf(),
        err,
    })?;
    let cfg: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| invalid(err.to_string()))?;
    from_decider_config(&cfg, layout).map_err(|err| RegistryError::Profile {
        path: path.to_path_buf(),
        err,
    })
}

/// The file format of `weights` by its magic bytes: `gguf`, else `None`.
pub fn weights_format(weights: &Path) -> Option<&'static str> {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(weights)
        .and_then(|mut f| f.read_exact(&mut magic))
        .ok()
        .and_then(|()| (&magic == b"GGUF").then_some("gguf"))
}

/// Today's UTC date, `YYYY-MM-DD`.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (y, m, d) = civil_from_days(i64::try_from(secs / 86_400).unwrap_or(0));
    format!("{y:04}-{m:02}-{d:02}")
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_724), (2026, 9, 28));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn models_toml_round_trips() {
        let mut profile = ModelProfile::stock(
            "qwen",
            Layout::Chat {
                head: vec![1, 2],
                tail: vec![3],
            },
        );
        profile
            .temperature_by_type
            .insert(ardana_core::AnswerType::Noul, 1.5);
        profile.release_date = Some("2026-09-24".into());
        let model = ResolvedModel {
            name: "qwen".into(),
            source: "hf.co/org/Repo-GGUF:Q4_0".into(),
            weights: "/w.gguf".into(),
            tokenizer: "/t/tokenizer.json".into(),
            runtime: "llama.cpp".into(),
            profile,
            pulled_at: "2026-09-28".into(),
        };
        let text = toml::to_string(&ModelsFile {
            models: vec![model.clone()],
        })
        .unwrap();
        let back: ModelsFile = toml::from_str(&text).unwrap();
        assert_eq!(back.models, vec![model], "{text}");
    }

    /// The library snapshot (C5), which cargo's `[env]` names as `ARDANA_LIBRARY` for every test.
    fn snapshot() -> Library {
        let path = PathBuf::from(
            std::env::var_os(library::LIBRARY_VAR).expect("cargo sets ARDANA_LIBRARY"),
        );
        Library::from_file(&path).unwrap_or_else(|err| panic!("{err}"))
    }

    /// A file of the pinned snapshot of `repo` in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
    fn hf_file(repo: &str, name: &str) -> PathBuf {
        let dir = PathBuf::from(std::env::var_os("HF_HOME").expect("cargo sets HF_HOME"))
            .join("hub")
            .join(format!("models--{}", repo.replace('/', "--")));
        let commit = std::fs::read_to_string(dir.join("refs/main"))
            .unwrap_or_else(|err| panic!("{}: {err}; run `cargo xtask fetch`", dir.display()));
        dir.join("snapshots").join(commit.trim()).join(name)
    }

    /// A hub cache in `$ARDANA_TMP/<test>` holding decider:2b's browser repository at `commit` with `files`: its text
    /// files as the repository has them (decider:2b's tokenizer and config, Qwen3.5's chat template), its graph and
    /// weights empty; and a client of that cache whose every Hub call goes to a closed port, not retried, so a lookup
    /// that leaves the cache fails. Returns the client and the snapshot directory.
    fn browser_cache(test: &str, commit: &str, files: &[&str]) -> (hf_hub::HFClient, PathBuf) {
        let cache = PathBuf::from(std::env::var_os("ARDANA_TMP").expect("cargo sets ARDANA_TMP"))
            .join(test);
        if cache.exists() {
            std::fs::remove_dir_all(&cache).unwrap();
        }
        let repo = cache.join("models--ardana-ai--decider-2b-ONNX");
        let snapshot = repo.join("snapshots").join(commit);
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::create_dir_all(repo.join("refs")).unwrap();
        std::fs::write(repo.join("refs").join("main"), commit).unwrap();
        for file in files {
            let source = match *file {
                "model.onnx" | "model.onnx.data" => None,
                ardana_core::chat::TEMPLATE_FILE => Some(hf_file("Qwen/Qwen3.5-0.8B", file)),
                _ => Some(hf_file("Mapika/decider-2b-GGUF", file)),
            };
            match source {
                Some(source) => {
                    std::fs::copy(&source, snapshot.join(file)).unwrap_or_else(|err| {
                        panic!("{}: {err}; run `cargo xtask fetch`", source.display())
                    });
                }
                None => std::fs::write(snapshot.join(file), b"").unwrap(),
            }
        }
        let client = hf_hub::HFClient::builder()
            .cache_dir(cache)
            .endpoint("http://127.0.0.1:9")
            .retry_max_attempts(0)
            .build()
            .unwrap();
        (client, snapshot)
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// A browser variant is taken from the cache only when the cache holds all of it: the decider family names no
    /// layout, so the profile of `decider:2b` comes from `decider_config.json`, and a cache without that file (or with a
    /// file it cannot read) sends the lookup to the Hub, or, offline, fails, rather than serve the stock profile.
    #[test]
    fn cached_browser_variants_are_complete() {
        let library = snapshot();
        let model = &library.browser("decider:2b").unwrap();
        let browser = model.size.browser.as_ref().unwrap();
        let reference = browser.repo.as_str();
        assert_eq!(reference, "hf.co/ardana-ai/decider-2b-ONNX");
        assert_eq!(model.size.layout, None);
        let commit = browser.commit.as_str();
        let lookup = |client: &hf_hub::HFClient, offline: bool| {
            block_on(browser_variant(
                &Hub::new(client.clone(), offline, false),
                model,
                browser,
            ))
        };
        let gone_online = |looked_up: &Result<BrowserModel, RegistryError>| matches!(looked_up, Err(RegistryError::Hub { repo, .. }) if repo == reference);
        // What `GET /v1/models` says of the variant (`x_browser_pulled`): whether the cache holds every file of it.
        let held = |client: &hf_hub::HFClient| {
            block_on(BrowserCache(Hub::new(client.clone(), true, false)).holds(model))
        };
        let parts = [
            TOKENIZER_FILE,
            ardana_core::chat::CONFIG_FILE,
            ardana_core::chat::TEMPLATE_FILE,
            "model.onnx",
            "model.onnx.data",
        ];
        let all: Vec<&str> = parts.iter().copied().chain([DECIDER_CONFIG]).collect();

        // The whole variant in the cache: read in place, with decider's profile.
        let (client, snapshot) = browser_cache("registry-browser-complete", commit, &all);
        let variant = lookup(&client, false).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(snapshot.join(DECIDER_CONFIG)).unwrap())
                .unwrap();
        let mut profile = from_decider_config(&config, Layout::Plain).unwrap();
        profile.release_date = Some(model.size.release_date.clone());
        assert_eq!(variant.profile, profile);
        assert_eq!(
            (variant.commit.as_str(), variant.reference.as_str()),
            (commit, reference)
        );
        assert_eq!(
            variant.file(TOKENIZER_FILE),
            Some(snapshot.join(TOKENIZER_FILE).as_path())
        );
        assert!(held(&client));

        // Without the config the cache holds part of the variant: the lookup goes to the Hub (a closed port here), and
        // offline, where the cache is all there is, it fails naming the file.
        let (client, _) = browser_cache("registry-browser-partial", commit, &parts);
        assert!(!held(&client));
        let looked_up = lookup(&client, false);
        assert!(gone_online(&looked_up), "{looked_up:?}");
        let looked_up = lookup(&client, true);
        assert!(
            matches!(&looked_up, Err(RegistryError::NotCached { what, .. })
                if *what == format!("{reference} file {DECIDER_CONFIG}")),
            "{looked_up:?}"
        );

        // A cached file that does not read sends the lookup to the Hub too, though the cache holds every file: the
        // files are found, not read, to answer for the list.
        let (client, snapshot) = browser_cache("registry-browser-unreadable", commit, &all);
        std::fs::write(snapshot.join(TOKENIZER_FILE), b"{").unwrap();
        let looked_up = lookup(&client, false);
        assert!(gone_online(&looked_up), "{looked_up:?}");
        assert!(held(&client));

        // A cache without the repository holds none of it.
        let (client, _) = browser_cache("registry-browser-none", commit, &all);
        let repository = client
            .cache_dir()
            .join("models--ardana-ai--decider-2b-ONNX");
        std::fs::remove_dir_all(repository).unwrap();
        assert!(!held(&client));
    }

    /// Q12: the unknown-model error lists the pulled names and points to the library's page, naming no library model.
    #[test]
    fn unknown_model_lists_pulled_names_and_the_library_page() {
        let err = RegistryError::UnknownModel {
            name: "x".into(),
            available: vec!["a".into(), "b".into()],
        };
        assert_eq!(
            err.to_string(),
            "no model named \"x\"; pulled: a, b; the library at https://ardana.ai/models/ is pulled on first use"
        );
        let err = RegistryError::UnknownModel {
            name: "x".into(),
            available: Vec::new(),
        };
        assert_eq!(
            err.to_string(),
            "no model named \"x\"; none pulled yet; the library at https://ardana.ai/models/ is pulled on first use"
        );
    }

    /// A pulled entry answers under its exact name whatever the library holds (Q25); another spelling of a library
    /// size goes through the library to its canonical name (Q10), and a name neither holds is unknown.
    #[test]
    fn names_are_pulled_entries_first() {
        let library = snapshot();
        let mut registry = Registry::open(Path::new("/nonexistent/home")).unwrap();
        assert!(matches!(
            registry.named("decider", &library),
            Ok(Named::Library(pick)) if pick.name() == "decider:2b"
        ));
        assert!(matches!(
            registry.named("decider:2b", &Library::empty()),
            Err(RegistryError::UnknownModel { .. })
        ));
        registry.insert(ResolvedModel {
            name: "decider:2b".into(),
            source: "hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf".into(),
            weights: "/w.gguf".into(),
            tokenizer: "/t/tokenizer.json".into(),
            runtime: "llama.cpp".into(),
            profile: ModelProfile::stock("decider-2b-v11", Layout::Plain),
            pulled_at: "2026-09-28".into(),
        });
        assert!(matches!(
            registry.named("decider:2b", &Library::empty()),
            Ok(Named::Pulled(model)) if model.name == "decider:2b"
        ));
        for spelling in ["decider", "Decider:LATEST", "Decider:2B-Q4_K_M"] {
            assert!(matches!(
                registry.named(spelling, &library),
                Ok(Named::Pulled(model)) if model.name == "decider:2b"
            ));
        }
        assert!(matches!(
            registry.named("decider:2b-Q8_0", &library),
            Ok(Named::Library(pick)) if pick.name() == "decider:2b-q8_0"
        ));
        assert!(matches!(
            registry.named("decider:9b", &library),
            Err(RegistryError::UnknownTag { family, .. }) if family == "decider"
        ));
    }
}
