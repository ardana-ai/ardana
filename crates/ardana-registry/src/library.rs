//! The built-in model library (`library.toml`, embedded): short names such as `decider-2b` for Hugging Face GGUF
//! repositories, which [`crate::pull`] resolves like the `hf.co/` reference they stand for. `<name>:<quant>` picks
//! another GGUF of the same repository, matched case-insensitively like an `hf.co/` quant.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::LayoutKind;
use crate::refs::{HF_PREFIX, HfFile, Ref, valid_name};

const LIBRARY_TOML: &str = include_str!("library.toml");

/// The models `library.toml` lists and the one requests use while nothing is pulled.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    /// The model a request without a model uses when the registry is empty.
    pub default: String,
    #[serde(rename = "model")]
    pub models: Vec<LibraryModel>,
}

/// One library entry; see `library.toml` for the fields.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryModel {
    pub name: String,
    /// `hf.co/<org>/<repo>`.
    pub weights: String,
    pub quant: String,
    /// The GGUF of `quant`, in bytes.
    pub size: u64,
    /// `YYYY-MM-DD`.
    pub release_date: String,
    #[serde(default)]
    pub tokenizer: Option<String>,
    #[serde(default)]
    pub layout: Option<LayoutKind>,
}

/// A library model and the quant a name picks from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPick<'a> {
    pub model: &'a LibraryModel,
    /// A quant other than the model's own, lowercased.
    quant: Option<String>,
}

impl LibraryPick<'_> {
    /// The registry name the pick is pulled as: the model's name, or `<name>:<quant>` for another quant.
    pub fn name(&self) -> String {
        match &self.quant {
            None => self.model.name.clone(),
            Some(quant) => format!("{}:{quant}", self.model.name),
        }
    }

    /// The `hf.co/<org>/<repo>:<quant>` reference the pick stands for.
    pub fn reference(&self) -> String {
        let quant = self.quant.as_deref().unwrap_or(&self.model.quant);
        format!("{}:{quant}", self.model.weights)
    }

    /// The download size in bytes, known for the model's own quant only.
    pub fn size(&self) -> Option<u64> {
        self.quant.is_none().then_some(self.model.size)
    }
}

/// The embedded library.
pub fn library() -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        Library::parse(LIBRARY_TOML)
            .expect("the embedded library.toml is valid (its unit test parses it)")
    })
}

impl Library {
    fn parse(text: &str) -> Result<Library, String> {
        let library: Library = toml::from_str(text).map_err(|err| err.to_string())?;
        for (i, model) in library.models.iter().enumerate() {
            let what = |msg: &str| format!("library model {:?}: {msg}", model.name);
            if !valid_name(&model.name) || model.name != model.name.to_lowercase() {
                return Err(what("a name is lowercase letters, digits, '.', '_' or '-'"));
            }
            if library.models[..i].iter().any(|m| m.name == model.name) {
                return Err(what("the name appears twice"));
            }
            for repo in std::iter::once(&model.weights).chain(&model.tokenizer) {
                // Checked for the prefix first: a bare name would make `Ref::parse` read this library.
                let hf = repo.starts_with(HF_PREFIX)
                    && matches!(Ref::parse(repo), Ok(Ref::Hf { file: None, .. }));
                if !hf {
                    return Err(what(&format!("{repo:?} is not {HF_PREFIX}<org>/<repo>")));
                }
            }
            let quant = Ref::parse(&format!("{}:{}", model.weights, model.quant));
            if !matches!(
                quant,
                Ok(Ref::Hf {
                    file: Some(HfFile::Quant(_)),
                    ..
                })
            ) {
                return Err(what("the quant is not a quant name"));
            }
        }
        if !library.models.iter().any(|m| m.name == library.default) {
            return Err(format!(
                "the default {:?} is not a library model",
                library.default
            ));
        }
        Ok(library)
    }

    /// The model names, in library order.
    pub fn names(&self) -> Vec<&str> {
        self.models.iter().map(|m| m.name.as_str()).collect()
    }

    /// The model requests use while the registry is empty.
    pub fn default_pick(&self) -> LibraryPick<'_> {
        self.find(&self.default)
            .expect("the library default is a library model (checked at parse)")
    }

    /// The library model `input` names, `<name>` or `<name>:<quant>`, both matched case-insensitively; the model's
    /// own quant is the bare name. `None` for anything else.
    pub fn find(&self, input: &str) -> Option<LibraryPick<'_>> {
        let (name, quant) = match input.split_once(':') {
            Some((name, quant)) => (name, Some(quant)),
            None => (input, None),
        };
        let model = self
            .models
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))?;
        let quant = match quant {
            None => None,
            Some(quant) if quant.eq_ignore_ascii_case(&model.quant) => None,
            Some(quant) if valid_name(quant) => Some(quant.to_lowercase()),
            Some(_) => return None,
        };
        Some(LibraryPick { model, quant })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_library_parses() {
        let library = Library::parse(LIBRARY_TOML).unwrap();
        assert_eq!(library.default, "decider-2b");
        assert_eq!(
            library.names(),
            ["decider-2b", "decider-4b", "qwen3.5-0.8b", "smollm3-3b"]
        );
    }

    #[test]
    fn invalid_libraries_are_refused() {
        let entry = |fields: &str| {
            format!(
                "default = \"a\"\n[[model]]\nname = \"a\"\nsize = 1\nrelease_date = \"2026-01-01\"\n{fields}"
            )
        };
        let ok = entry("weights = \"hf.co/o/r\"\nquant = \"Q4_0\"");
        assert!(Library::parse(&ok).is_ok());
        for bad in [
            entry("weights = \"o/r\"\nquant = \"Q4_0\""),
            entry("weights = \"hf.co/o/r:Q4_0\"\nquant = \"Q4_0\""),
            entry("weights = \"hf.co/o/r\"\nquant = \"Q4 0\""),
            entry("weights = \"hf.co/o/r\"\nquant = \"Q4_0\"\ntokenizer = \"ollama:x\""),
            ok.replace("default = \"a\"", "default = \"b\""),
            ok.replace("name = \"a\"", "name = \"A\""),
        ] {
            assert!(Library::parse(&bad).is_err(), "{bad}");
        }
    }
}
