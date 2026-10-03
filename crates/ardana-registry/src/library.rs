//! The built-in model library (`library.toml`, embedded): short names such as `decider-2b` for Hugging Face GGUF
//! repositories, which [`crate::pull`] resolves like the `hf.co/` reference they stand for. `<name>:<quant>` picks
//! another GGUF of the same repository, matched case-insensitively like an `hf.co/` quant. A model may also have a
//! browser variant, ONNX weights onnxruntime-web runs in a visitor's tab ([`BrowserWeights`]).

use std::sync::OnceLock;

use serde::Deserialize;

use crate::LayoutKind;
use crate::refs::{HF_PREFIX, HfFile, Ref, valid_name};

const LIBRARY_TOML: &str = include_str!("library.toml");

/// The quantizations `cargo xtask onnx convert` builds browser variants in.
const BROWSER_QUANTS: [&str; 2] = ["int4", "int8"];

/// The models `library.toml` lists, the one requests use while nothing is pulled and the first browser model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    /// The model a request without a model uses when the registry is empty.
    pub default: String,
    /// The library model with a browser variant that a playground offers first.
    #[serde(default)]
    pub browser_default: Option<String>,
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
    /// `hf.co/<org>/<repo>@<commit>`, the checkpoint `cargo xtask onnx convert` builds the browser variant (and a GGUF
    /// ardana-ai hosts) from.
    #[serde(default)]
    pub source: Option<String>,
    /// The browser variant (`[model.browser]`).
    #[serde(default)]
    pub browser: Option<BrowserWeights>,
}

/// A library model's ONNX weights for onnxruntime-web, built by `cargo xtask onnx convert`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserWeights {
    /// `hf.co/<org>/<repo>`, the ONNX repository.
    pub weights: String,
    /// The weight quantization: `int4` or `int8`.
    pub quant: String,
    /// The bytes a browser downloads: `model.onnx`, `model.onnx.data` and `tokenizer.json`.
    pub size: u64,
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
            let browser = model.browser.as_ref().map(|b| &b.weights);
            for repo in std::iter::once(&model.weights)
                .chain(&model.tokenizer)
                .chain(browser)
            {
                if !hf_repo(repo) {
                    return Err(what(&format!("{repo:?} is not {HF_PREFIX}<org>/<repo>")));
                }
            }
            if let Some(source) = &model.source {
                let pinned = source.split_once('@').is_some_and(|(repo, commit)| {
                    hf_repo(repo)
                        && commit.len() == 40
                        && commit
                            .bytes()
                            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                });
                if !pinned {
                    return Err(what(&format!(
                        "the source {source:?} is not {HF_PREFIX}<org>/<repo>@<commit>"
                    )));
                }
            }
            if let Some(browser) = &model.browser
                && !BROWSER_QUANTS.contains(&browser.quant.as_str())
            {
                return Err(what(&format!(
                    "the browser quant {:?} is not one of {}",
                    browser.quant,
                    BROWSER_QUANTS.join(", ")
                )));
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
        if let Some(name) = &library.browser_default
            && !library
                .models
                .iter()
                .any(|m| &m.name == name && m.browser.is_some())
        {
            return Err(format!(
                "the browser default {name:?} is not a library model with a browser variant"
            ));
        }
        Ok(library)
    }

    /// The model names, in library order.
    pub fn names(&self) -> Vec<&str> {
        self.models.iter().map(|m| m.name.as_str()).collect()
    }

    /// The library model named exactly `name` and its browser variant, when it has one.
    pub fn browser(&self, name: &str) -> Option<(&LibraryModel, &BrowserWeights)> {
        let model = self.models.iter().find(|m| m.name == name)?;
        Some((model, model.browser.as_ref()?))
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

/// Whether `repo` is `hf.co/<org>/<repo>`, without a quant or file. Checked for the prefix first: a bare name would
/// make `Ref::parse` read this library.
fn hf_repo(repo: &str) -> bool {
    repo.starts_with(HF_PREFIX) && matches!(Ref::parse(repo), Ok(Ref::Hf { file: None, .. }))
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
            [
                "decider-2b",
                "decider-0.8b",
                "decider-4b",
                "qwen3.5-0.8b",
                "smollm3-3b"
            ]
        );
    }

    /// R1.3: the browser variants, the checkpoints they are built from and the browser default; a browser table
    /// names its ONNX repository, quant and download size.
    #[test]
    fn library_browser() {
        let library = Library::parse(LIBRARY_TOML).unwrap();
        assert_eq!(library.browser_default.as_deref(), Some("decider-0.8b"));
        let browser: Vec<(&str, &str, &str, Option<&str>)> = library
            .models
            .iter()
            .filter_map(|m| {
                let browser = m.browser.as_ref()?;
                assert!(browser.size > 0, "{}", m.name);
                Some((
                    m.name.as_str(),
                    browser.weights.as_str(),
                    browser.quant.as_str(),
                    m.source.as_deref(),
                ))
            })
            .collect();
        assert_eq!(
            browser,
            [
                (
                    "decider-2b",
                    "hf.co/ardana-ai/decider-2b-ONNX",
                    "int4",
                    Some("hf.co/Mapika/decider-2b@533964dae8be954c5b5e19fa4948e48408094c1e")
                ),
                (
                    "decider-0.8b",
                    "hf.co/ardana-ai/decider-0.8b-ONNX",
                    "int4",
                    Some("hf.co/Mapika/decider-0.8b@a0a01d6f8135298f400a8c856b355793012ae971")
                ),
                (
                    "qwen3.5-0.8b",
                    "hf.co/ardana-ai/qwen3.5-0.8b-ONNX",
                    "int8",
                    Some("hf.co/Qwen/Qwen3.5-0.8B@2fc06364715b967f1860aea9cf38778875588b17")
                ),
            ]
        );
        // decider-0.8b's GGUF is ardana-ai's own, built from the same checkpoint.
        let decider = library.find("decider-0.8b").unwrap();
        assert_eq!(
            decider.reference(),
            "hf.co/ardana-ai/decider-0.8b-GGUF:Q8_0"
        );
        assert!(decider.size().is_some_and(|size| size > 0));
        for name in ["decider-4b", "smollm3-3b"] {
            let model = library.find(name).unwrap().model;
            assert_eq!((&model.browser, &model.source), (&None, &None), "{name}");
        }

        const COMMIT: &str = "a0a01d6f8135298f400a8c856b355793012ae971";
        let entry = |browser: &str| {
            format!(
                "default = \"a\"\nbrowser_default = \"a\"\n[[model]]\nname = \"a\"\nweights = \"hf.co/o/r\"\n\
                 quant = \"Q4_0\"\nsize = 1\nrelease_date = \"2026-01-01\"\nsource = \"hf.co/o/s@{COMMIT}\"\n\
                 [model.browser]\n{browser}"
            )
        };
        let table = "weights = \"hf.co/o/r-ONNX\"\nquant = \"int4\"\nsize = 1";
        let ok = entry(table);
        assert!(Library::parse(&ok).is_ok());
        let err =
            Library::parse(&entry("weights = \"hf.co/o/r-ONNX\"\nquant = \"int4\"")).unwrap_err();
        assert!(err.contains("missing field `size`"), "{err}");
        for bad in [
            entry("weights = \"o/r-ONNX\"\nquant = \"int4\"\nsize = 1"),
            entry("weights = \"hf.co/o/r-ONNX:int4\"\nquant = \"int4\"\nsize = 1"),
            entry("weights = \"hf.co/o/r-ONNX\"\nquant = \"Q4_0\"\nsize = 1"),
            entry(&format!("{table}\nfile = \"model.onnx\"")),
            ok.replace("browser_default = \"a\"", "browser_default = \"b\""),
            ok.replace(&format!("[model.browser]\n{table}"), ""),
            ok.replace(COMMIT, &COMMIT[..7]),
            ok.replace(COMMIT, &COMMIT.to_uppercase()),
            ok.replace("hf.co/o/s@", "hf.co/o/s:"),
        ] {
            assert!(Library::parse(&bad).is_err(), "{bad}");
        }
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
