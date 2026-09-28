//! The local Ollama store, read in place and never written: `manifests/registry.ollama.ai/<namespace>/<name>/<tag>`
//! holds a Docker v2 manifest whose `application/vnd.ollama.image.model` layer is the GGUF at
//! `blobs/sha256-<hex>`.

use std::path::PathBuf;

use serde::Deserialize;

use crate::RegistryError;

/// The variable naming the store; without it the store is `~/.ollama/models`.
pub const MODELS_VAR: &str = "OLLAMA_MODELS";
/// The registry host every `ollama:` reference reads.
pub const HOST: &str = "registry.ollama.ai";
/// The media type of the manifest layer holding the weights.
pub const MODEL_MEDIA_TYPE: &str = "application/vnd.ollama.image.model";

#[derive(Deserialize)]
struct Manifest {
    layers: Vec<Layer>,
}

#[derive(Deserialize)]
struct Layer {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
}

/// `$OLLAMA_MODELS`, else `~/.ollama/models`.
pub fn models_dir() -> Result<PathBuf, RegistryError> {
    if let Some(dir) = std::env::var_os(MODELS_VAR) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| RegistryError::Invalid {
        what: "the Ollama store".into(),
        msg: format!("neither {MODELS_VAR} nor HOME is set"),
    })?;
    Ok(PathBuf::from(home).join(".ollama/models"))
}

/// The GGUF blob of `ollama:<namespace>/<name>:<tag>` (named `source` in messages).
pub fn blob(
    source: &str,
    namespace: &str,
    name: &str,
    tag: &str,
) -> Result<PathBuf, RegistryError> {
    let store = models_dir()?;
    let manifest_path = store
        .join("manifests")
        .join(HOST)
        .join(namespace)
        .join(name)
        .join(tag);
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(RegistryError::Invalid {
                what: source.to_string(),
                msg: format!(
                    "not in the Ollama store at {} (no manifest {}); run `ollama pull {name}:{tag}` first",
                    store.display(),
                    manifest_path.display()
                ),
            });
        }
        Err(err) => {
            return Err(RegistryError::Io {
                path: manifest_path,
                err,
            });
        }
    };
    let invalid = |msg: String| RegistryError::Invalid {
        what: format!("{source} manifest {}", manifest_path.display()),
        msg,
    };
    let manifest: Manifest = serde_json::from_str(&text).map_err(|err| invalid(err.to_string()))?;
    let layer = manifest
        .layers
        .iter()
        .find(|l| l.media_type == MODEL_MEDIA_TYPE)
        .ok_or_else(|| invalid(format!("no {MODEL_MEDIA_TYPE} layer")))?;
    let hex = layer
        .digest
        .strip_prefix("sha256:")
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            invalid(format!(
                "the model layer digest {:?} is not sha256:<hex>",
                layer.digest
            ))
        })?;
    let blob = store.join("blobs").join(format!("sha256-{hex}"));
    if !blob.is_file() {
        return Err(invalid(format!(
            "its model blob {} is missing",
            blob.display()
        )));
    }
    Ok(blob)
}
