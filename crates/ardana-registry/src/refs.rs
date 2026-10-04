//! Model references (Q3): `hf.co/<org>/<repo>[:<quant>]`, `hf.co/<org>/<repo>:<file>.gguf`,
//! `ollama:[<namespace>/]<name>[:<tag>]` and local paths. Library names (`decider-2b`) are resolved before these, by
//! [`crate::library`]; the error of a bare name points to the library's page and names no model (Q11).

use std::path::{Path, PathBuf};

/// The prefix of a Hugging Face repository reference.
pub const HF_PREFIX: &str = "hf.co/";
/// The prefix of a reference into the local Ollama store.
pub const OLLAMA_PREFIX: &str = "ollama:";
/// The quant an `hf.co/` reference without one picks (Ollama's default for `hf.co/` refs).
pub const DEFAULT_QUANT: &str = "Q4_K_M";
/// The Ollama namespace of a reference without one.
pub const OLLAMA_NAMESPACE: &str = "library";
/// The Ollama tag of a reference without one.
pub const OLLAMA_TAG: &str = "latest";

const GRAMMAR: &str = "use a library model <name>[:<quant>] (https://ardana.ai/models/), hf.co/<org>/<repo>[:<quant>], \
                       hf.co/<org>/<repo>:<file>.gguf, ollama:[<namespace>/]<name>[:<tag>] or a path to a local file";

/// A parsed model reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// A Hugging Face model repository, optionally narrowed to one GGUF.
    Hf {
        org: String,
        repo: String,
        file: Option<HfFile>,
    },
    /// A model in the local Ollama store (`registry.ollama.ai`), read in place.
    Ollama {
        namespace: String,
        name: String,
        tag: String,
    },
    /// A file (or, for a tokenizer, a directory) on disk.
    Local(PathBuf),
}

/// Which GGUF of an `hf.co/` repository a reference names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HfFile {
    /// The GGUF whose name ends in `-<quant>` or `.<quant>`, matched case-insensitively.
    Quant(String),
    /// The GGUF at this path in the repository.
    Name(String),
}

/// A string that is none of the reference forms.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{input:?} is not a model reference: {reason}; {GRAMMAR}")]
pub struct RefError {
    pub input: String,
    pub reason: String,
}

impl Ref {
    pub fn parse(input: &str) -> Result<Ref, RefError> {
        let fail = |reason: String| RefError {
            input: input.to_string(),
            reason,
        };
        if input.is_empty() {
            return Err(fail("it is empty".into()));
        }
        if input.trim() != input {
            return Err(fail("it has leading or trailing whitespace".into()));
        }
        if let Some(rest) = input.strip_prefix(HF_PREFIX) {
            return parse_hf(rest).map_err(fail);
        }
        if let Some(rest) = input.strip_prefix(OLLAMA_PREFIX) {
            return parse_ollama(rest).map_err(fail);
        }
        for web in [
            "https://",
            "http://",
            "huggingface.co/",
            "www.huggingface.co/",
        ] {
            if input.starts_with(web) {
                return Err(fail(format!(
                    "write Hugging Face repositories as {HF_PREFIX}<org>/<repo>"
                )));
            }
        }
        if input.contains('/') || input.to_ascii_lowercase().ends_with(".gguf") {
            return Ok(Ref::Local(PathBuf::from(input)));
        }
        let library = crate::library::MODELS_PAGE;
        if valid_name(input.split_once(':').map_or(input, |(name, _)| name)) {
            return Err(fail(format!(
                "it is neither a library model ({library}), {HF_PREFIX}, {OLLAMA_PREFIX} nor a path (for an Ollama \
                 model write {OLLAMA_PREFIX}{input})"
            )));
        }
        Err(fail(format!(
            "it is neither a library model ({library}), {HF_PREFIX}, {OLLAMA_PREFIX} nor a path"
        )))
    }

    /// The registry name `ardana pull` gives the model without `--name`: the repository name lowercased minus a
    /// `-GGUF` suffix, the Ollama model name without its tag, or the local file's stem, lowercased.
    pub fn default_name(&self) -> String {
        match self {
            Ref::Hf { repo, .. } => {
                let repo = repo.to_lowercase();
                repo.strip_suffix("-gguf").unwrap_or(&repo).to_string()
            }
            Ref::Ollama { name, .. } => name.to_lowercase(),
            Ref::Local(path) => path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().to_lowercase()),
        }
    }
}

fn parse_hf(rest: &str) -> Result<Ref, String> {
    let (repo_path, pick) = match rest.split_once(':') {
        Some((path, pick)) => (path, Some(pick)),
        None => (rest, None),
    };
    let segments: Vec<&str> = repo_path.split('/').collect();
    let [org, repo] = segments[..] else {
        return Err(format!(
            "{HF_PREFIX} is followed by {repo_path:?}, not <org>/<repo>"
        ));
    };
    for (what, segment) in [("organisation", org), ("repository", repo)] {
        if !valid_name(segment) {
            return Err(format!(
                "the {what} {segment:?} must be letters, digits, '.', '_' or '-'"
            ));
        }
    }
    let file = match pick {
        None => None,
        Some("") => return Err("the quant after ':' is empty".into()),
        Some(name) if name.to_ascii_lowercase().ends_with(".gguf") => {
            let bad = name.starts_with('/')
                || name.split('/').any(|part| part.is_empty() || part == "..")
                || name.chars().any(char::is_whitespace);
            if bad {
                return Err(format!("the file {name:?} is not a path in the repository"));
            }
            Some(HfFile::Name(name.to_string()))
        }
        Some(quant) => {
            if !valid_name(quant) {
                return Err(format!(
                    "the quant {quant:?} must be letters, digits, '.', '_' or '-' (or a file ending in .gguf)"
                ));
            }
            Some(HfFile::Quant(quant.to_string()))
        }
    };
    Ok(Ref::Hf {
        org: org.to_string(),
        repo: repo.to_string(),
        file,
    })
}

fn parse_ollama(rest: &str) -> Result<Ref, String> {
    let (path, tag) = match rest.split_once(':') {
        Some((path, tag)) => (path, tag),
        None => (rest, OLLAMA_TAG),
    };
    let segments: Vec<&str> = path.split('/').collect();
    let (namespace, name) = match segments[..] {
        [name] => (OLLAMA_NAMESPACE, name),
        [namespace, name] => (namespace, name),
        _ => {
            return Err(format!(
                "{OLLAMA_PREFIX} is followed by {path:?}, not [<namespace>/]<name>"
            ));
        }
    };
    for (what, part) in [("namespace", namespace), ("name", name), ("tag", tag)] {
        if !valid_name(part) {
            return Err(format!(
                "the {what} {part:?} must be letters, digits, '.', '_' or '-'"
            ));
        }
    }
    Ok(Ref::Ollama {
        namespace: namespace.to_string(),
        name: name.to_string(),
        tag: tag.to_string(),
    })
}

/// A non-empty name of ASCII letters, digits, `.`, `_` and `-` that is not `.` or `..`.
pub(crate) fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Whether the GGUF at `file` (a repository path) is the `quant` one: its stem ends in `-<quant>` or `.<quant>`,
/// case-insensitively.
pub fn matches_quant(file: &str, quant: &str) -> bool {
    let name = Path::new(file)
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().to_lowercase());
    let Some(stem) = name.strip_suffix(".gguf") else {
        return false;
    };
    let quant = quant.to_lowercase();
    stem.strip_suffix(&quant)
        .is_some_and(|head| head.ends_with('-') || head.ends_with('.'))
}

/// Whether the GGUF at `file` is a companion of the model GGUF at `model`: the model's file name after a prefix, as
/// in `mmproj-<model>.gguf`, `mtp-<model>.gguf` and `dflash-<model>.gguf`.
pub fn companion_of(file: &str, model: &str) -> bool {
    let name = |path: &str| {
        Path::new(path)
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().to_lowercase())
    };
    let (file, model) = (name(file), name(model));
    file.strip_suffix(&model)
        .is_some_and(|prefix| prefix.ends_with('-'))
}
