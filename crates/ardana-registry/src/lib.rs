//! Model references, the model library, `models.toml`, and Hugging Face and Ollama resolution.
//!
//! `ardana pull <ref>` resolves a library name ([`library`]) or a reference ([`refs::Ref`]) to weights and a
//! tokenizer in place (the hub cache, the Ollama store or a local file; nothing is copied), derives the model's
//! profile (from `decider_config.json` when the weights come with one, else [`ModelProfile::stock`] in the chat
//! layout) and records the result as one entry of `$ARDANA_HOME/models.toml` ([`Registry`]). The server and
//! `ardana run <name>` look names up with [`Registry::named`]: a pulled entry, else a library model they pull first.

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
use crate::library::{LibraryPick, library};
use crate::refs::{DEFAULT_QUANT, HfFile, Ref, RefError, matches_quant};

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

/// One registry entry: a named model resolved to its files, runtime and profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedModel {
    pub name: String,
    /// The reference it was pulled from (a local path is recorded absolute).
    pub source: String,
    /// The weights, read in place: a hub cache snapshot path, an Ollama blob or a local file.
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
    #[error("{what}: {msg}")]
    Invalid { what: String, msg: String },
    #[error("{}: {err}", .path.display())]
    Io { path: PathBuf, err: std::io::Error },
    #[error("{repo}: {err}")]
    Hub { repo: String, err: hf_hub::HFError },
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
        "no model named {name:?}; {pulled}; library, pulled on first use: {}",
        library().names().join(", ")
    )
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
        let home = std::env::var_os("HOME").ok_or_else(|| RegistryError::Invalid {
            what: "the Ardana home".into(),
            msg: format!("neither {HOME_VAR} nor HOME is set"),
        })?;
        Ok(PathBuf::from(home).join(".ardana"))
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

    /// What `name` means here: the entry of that name, else the library model it names (which may be pulled under
    /// its canonical name, `decider-2b` for `decider-2b:Q4_K_M`), else an [`RegistryError::UnknownModel`] listing
    /// both.
    pub fn named(&self, name: &str) -> Result<Named<'_>, RegistryError> {
        if let Some(model) = self.models.iter().find(|m| m.name == name) {
            return Ok(Named::Pulled(model));
        }
        let Some(pick) = library().find(name) else {
            return Err(RegistryError::UnknownModel {
                name: name.to_string(),
                available: self.names(),
            });
        };
        let canonical = pick.name();
        Ok(match self.models.iter().find(|m| m.name == canonical) {
            Some(model) => Named::Pulled(model),
            None => Named::Library(pick),
        })
    }

    /// When `models.toml` was last written; `None` while it does not exist.
    pub fn modified(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok()
    }

    /// The entry named `name`, with its weights and tokenizer still in place.
    pub fn resolve(&self, name: &str) -> Result<ResolvedModel, RegistryError> {
        let model = self.entry(name)?;
        for path in [&model.weights, &model.tokenizer] {
            if !path.is_file() {
                return Err(RegistryError::Missing {
                    name: model.name.clone(),
                    reference: model.source.clone(),
                    path: path.clone(),
                });
            }
        }
        Ok(model.clone())
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
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |err| RegistryError::Io { path, err }
        };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io(dir))?;
        }
        let file = ModelsFile {
            models: self.models.clone(),
        };
        let text = toml::to_string(&file).map_err(|err| RegistryError::Invalid {
            what: self.path.display().to_string(),
            msg: err.to_string(),
        })?;
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(io(&tmp))?;
        std::fs::rename(&tmp, &self.path).map_err(io(&self.path))
    }
}

/// What a model name means in a registry ([`Registry::named`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Named<'a> {
    /// A pulled entry.
    Pulled(&'a ResolvedModel),
    /// A library model not pulled yet; [`pull`] its [`LibraryPick::name`].
    Library(LibraryPick<'static>),
}

/// The prompt layout a model is read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutKind {
    /// decider's plain layout, for decider-format models.
    Plain,
    /// The tokenizer's chat template, for stock instruct models read zero-shot.
    Chat,
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

/// Resolves `reference`, a library name or a reference, to a registry entry, downloading Hugging Face files into
/// the hub cache unless `HF_HUB_OFFLINE` is set. A library name pulls the reference it stands for under its
/// [`LibraryPick::name`], with the library's tokenizer, layout and release date where `opts` and the weights give
/// none. Nothing is written to the registry; the caller [`Registry::insert`]s the result.
pub async fn pull(
    reference: &str,
    opts: &PullOptions,
    runtimes: &Runtimes,
) -> Result<ResolvedModel, RegistryError> {
    let Some(pick) = library().find(reference) else {
        return pull_ref(reference, opts, runtimes).await;
    };
    let opts = PullOptions {
        name: Some(opts.name.clone().unwrap_or_else(|| pick.name())),
        tokenizer: opts
            .tokenizer
            .clone()
            .or_else(|| pick.model.tokenizer.clone()),
        layout: opts.layout.or(pick.model.layout),
        progress: opts.progress,
    };
    let mut model = pull_ref(&pick.reference(), &opts, runtimes).await?;
    model
        .profile
        .release_date
        .get_or_insert_with(|| pick.model.release_date.clone());
    Ok(model)
}

async fn pull_ref(
    reference: &str,
    opts: &PullOptions,
    runtimes: &Runtimes,
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
            let snapshot = hub.snapshot(org, repo).await?;
            let gguf = pick_gguf(&snapshot, file.as_ref())?;
            let weights = hub.file(&snapshot, &gguf).await?;
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
        (Some(tokenizer), _) => tokenizer_ref(&hub, tokenizer).await?,
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
    let layout = opts.layout.unwrap_or(match config {
        Some(_) => LayoutKind::Plain,
        None => LayoutKind::Chat,
    });
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

/// The GGUF of `snapshot` that `file` names; without `file`, the [`DEFAULT_QUANT`] one.
fn pick_gguf(snapshot: &Snapshot, file: Option<&HfFile>) -> Result<String, RegistryError> {
    let ggufs: Vec<&String> = snapshot
        .files
        .iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".gguf"))
        .collect();
    let invalid = |msg: String| RegistryError::Invalid {
        what: snapshot.id(),
        msg,
    };
    let listing = || {
        if ggufs.is_empty() {
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
    let matches: Vec<&String> = ggufs
        .iter()
        .copied()
        .filter(|f| matches_quant(f, quant))
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

/// The `tokenizer.json` a `--tokenizer` reference names: an `hf.co/<org>/<repo>` repository, or a local file or
/// directory.
async fn tokenizer_ref(hub: &Hub, reference: &str) -> Result<PathBuf, RegistryError> {
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
            let snapshot = hub.snapshot(&org, &repo).await?;
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

    #[test]
    fn unknown_model_lists_pulled_and_library_names() {
        let err = RegistryError::UnknownModel {
            name: "x".into(),
            available: vec!["a".into(), "b".into()],
        };
        assert_eq!(
            err.to_string(),
            "no model named \"x\"; pulled: a, b; library, pulled on first use: decider-2b, decider-4b, \
             qwen3.5-0.8b, smollm3-3b"
        );
        let err = RegistryError::UnknownModel {
            name: "x".into(),
            available: Vec::new(),
        };
        assert!(
            err.to_string().starts_with(
                "no model named \"x\"; none pulled yet; library, pulled on first use: decider-2b,"
            ),
            "{err}"
        );
    }
}
