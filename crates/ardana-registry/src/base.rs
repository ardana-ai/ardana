//! The base `ardana finetune` trains from: a library size and the transformers checkpoint the library pins for it (a
//! decider size's `source`, an instruct size's `tokenizer`, which is the whole repository at a commit). [`base`] reads
//! what the fine-tune needs before any weight, the checkpoint's listing and the tokenizer and profile `ardana pull`
//! derives for the size, and refuses a decision model (THX-01) from its listing; [`Base::download`] then fetches the
//! checkpoint into the hub cache.

use std::path::PathBuf;

use ardana_core::{LayoutKind, ModelProfile, RepoPin, thx01};

use crate::hub::{Hub, Snapshot};
use crate::library::{Library, MODELS_PAGE};
use crate::refs::{self, Ref};
use crate::{
    DECIDER_CONFIG, RegistryError, default_layout, hf_tokenizer, load_tokenizer, read_profile,
    safetensors, tokenizer_ref,
};

/// A library size as `ardana finetune` reads it before any weight.
#[derive(Debug, Clone)]
pub struct Base {
    /// The size's canonical name.
    pub name: String,
    /// The checkpoint, `hf.co/<org>/<repo>` at its commit.
    pub checkpoint: RepoPin,
    /// The quant of the size's default GGUF.
    pub quant: String,
    /// The size's license, else its family's.
    pub license: Option<String>,
    /// The layout the size is read in.
    pub layout: LayoutKind,
    /// The `tokenizer.json` the size reads with, in the hub cache, its chat template files beside it.
    pub tokenizer: PathBuf,
    /// The profile `ardana pull <name>` derives.
    pub profile: ModelProfile,
    /// The checkpoint's files.
    snapshot: Snapshot,
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

/// The base `input` names in `library`: the size it names, read through `hub` from the environment (offline under
/// `HF_HUB_OFFLINE`), with no weight downloaded. A size the library pins no checkpoint for, a checkpoint whose listing
/// holds `rl_agent_config.json` (a decision model, Q17) and anything but a library size are refused; an
/// `hf.co/<org>/<repo>` reference is listed first, so a decision model is named one.
pub async fn base(input: &str, library: &Library, progress: bool) -> Result<Base, RegistryError> {
    let hub = Hub::from_env(progress)?;
    let Some(pick) = library.find(input)? else {
        if let Ok(Ref::Hf {
            org,
            repo,
            file: None,
        }) = Ref::parse(input)
        {
            undecided(&hub.snapshot(&org, &repo, None).await?)?;
        }
        return Err(RegistryError::Invalid {
            what: format!("the base {input}"),
            msg: format!("ardana finetune trains a library size, <family>:<size> ({MODELS_PAGE})"),
        });
    };
    let name = pick.name();
    let size = &pick.size;
    let checkpoint = size
        .source
        .clone()
        .or_else(|| size.tokenizer.clone())
        .ok_or_else(|| RegistryError::Invalid {
            what: name.clone(),
            msg: "the library pins no transformers checkpoint for it to train from".into(),
        })?;
    let Ref::Hf {
        org,
        repo,
        file: None,
    } = Ref::parse(&checkpoint.repo)?
    else {
        return Err(RegistryError::Invalid {
            what: checkpoint.repo.clone(),
            msg: format!(
                "a checkpoint is a whole repository, {}<org>/<repo>",
                refs::HF_PREFIX
            ),
        });
    };
    let snapshot = hub.snapshot(&org, &repo, Some(&checkpoint.commit)).await?;
    undecided(&snapshot)?;

    // The GGUF repository is read, at its commit, for what `ardana pull` takes from it besides the weights: its
    // tokenizer when the size pins none, and `decider_config.json` when the size names no layout (Q7).
    let gguf = if size.tokenizer.is_none() || size.layout.is_none() {
        let Ref::Hf { org, repo, .. } = Ref::parse(&size.gguf.repo)? else {
            return Err(RegistryError::Invalid {
                what: size.gguf.repo.clone(),
                msg: "a GGUF repository is an hf.co/ reference".into(),
            });
        };
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
        Some(pin) => tokenizer_ref(&hub, &pin.repo, Some(&pin.commit)).await?,
        None => {
            let gguf = gguf
                .as_ref()
                .expect("a size without a tokenizer pin has its GGUF repository read");
            hf_tokenizer(&hub, gguf).await?
        }
    };
    let layout = size
        .layout
        .unwrap_or_else(|| default_layout(config.as_deref()));
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
        quant: pick.quant.quant.clone(),
        license: size.license.clone(),
        layout,
        tokenizer,
        profile,
        snapshot,
    })
}

/// Refuses a checkpoint whose listing holds `rl_agent_config.json`: a THX-01 decision model, which reads no prompt
/// row and so trains on none (Q17).
fn undecided(snapshot: &Snapshot) -> Result<(), RegistryError> {
    if snapshot.has(thx01::DECISION_CONFIG) {
        return Err(RegistryError::Invalid {
            what: snapshot.id(),
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
    /// Downloads the checkpoint at its commit into the hub cache (unless the cache holds it, or offline from the cache
    /// alone): `config.json`, its safetensors and every other file at the repository's root but weights of other
    /// formats. Returns its snapshot directory.
    pub async fn download(&self, progress: bool) -> Result<PathBuf, RegistryError> {
        use ardana_core::snapshot::CONFIG;
        let hub = Hub::from_env(progress)?;
        let snapshot = &self.snapshot;
        if !snapshot.has(CONFIG) {
            return Err(RegistryError::Invalid {
                what: snapshot.id(),
                msg: format!("it has no {CONFIG}, so it is no transformers checkpoint"),
            });
        }
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
