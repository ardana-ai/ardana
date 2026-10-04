//! The model library document (C1): `https://ardana.ai/models.json`, the index of every library model, and
//! `https://ardana.ai/models/<name>.json`, one model's manifest (C2). The `ardana` binary, the standalone playground
//! and xtask read it at run time; no copy is compiled into a binary.
//!
//! The document is `schema` 1 and changes additively (Q12): a reader ignores fields it does not know, skips an entry it
//! cannot read or whose `min_version` is above its own version, and refuses another `schema` value with an error that
//! says to update `ardana`. [`LibraryDocument::parse`] and [`LibraryManifest::parse`] read that way.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The one schema this reader reads.
pub const LIBRARY_SCHEMA: u32 = 1;

/// The whole library: `GET <origin>/models.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryDocument {
    pub schema: u32,
    /// The model a request without a model uses when nothing is pulled.
    pub default: String,
    /// The library model with a browser variant that a playground offers first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_default: Option<String>,
    pub models: Vec<LibraryEntry>,
}

/// One model's manifest: `GET <origin>/models/<name>.json`, the same entry object as the index holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryManifest {
    pub schema: u32,
    pub model: LibraryEntry,
}

/// One library model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    /// Lowercase letters, digits, `.`, `_` and `-`.
    pub name: String,
    /// `decider` or `instruct`.
    pub kind: String,
    /// One or two sentences for the pages.
    pub summary: String,
    /// The checkpoint the model is based on, `<org>/<repo>`.
    pub base: String,
    /// The parameter count, as `1.9B`.
    pub params: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// `hf.co/<org>/<repo>`, the GGUF repository.
    pub weights: String,
    /// The 40-hex commit of `weights` the model is read at.
    pub commit: String,
    /// The default quant; `<name>:<quant>` picks another GGUF of the same repository.
    pub quant: String,
    /// The GGUF of `quant`, in bytes.
    pub size: u64,
    /// `YYYY-MM-DD`.
    pub release_date: String,
    /// `hf.co/<org>/<repo>`, when the GGUF repository carries no tokenizer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokenizer: Option<String>,
    /// The commit of `tokenizer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokenizer_commit: Option<String>,
    /// The prompt layout; absent, a pull infers it (Q18).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutKind>,
    /// `hf.co/<org>/<repo>@<commit>`, the checkpoint the browser variant (and a GGUF ardana-ai hosts) is built from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The least `ardana` version that reads this entry; older versions skip it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    /// Every quant the repository holds, for the pages.
    #[serde(default)]
    pub tags: Vec<LibraryTag>,
    /// The browser variant, ONNX weights a visitor's tab runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserEntry>,
}

/// One quant of a library model's repository and its GGUF's size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryTag {
    pub tag: String,
    pub size: u64,
}

/// A library model's ONNX weights for onnxruntime-web, built by `cargo xtask onnx convert`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserEntry {
    /// `hf.co/<org>/<repo>`, the ONNX repository.
    pub weights: String,
    /// The 40-hex commit of the files a tab downloads.
    pub commit: String,
    /// The weight quantization: `int4` or `int8`.
    pub quant: String,
    /// The bytes a browser downloads: `model.onnx`, `model.onnx.data` and `tokenizer.json`.
    pub size: u64,
    /// The profile the tab reads the files with, the object `GET /v1/browser/<name>/profile` answers.
    pub profile: Value,
}

/// The prompt layout a model is read in: a library entry's `layout` and `ardana pull --layout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutKind {
    /// decider's plain layout, for decider-format models.
    Plain,
    /// The tokenizer's chat template, for stock instruct models read zero-shot.
    Chat,
}

/// Why a document is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryError {
    /// Not the document's JSON.
    Json(String),
    /// A schema this reader does not read.
    Schema(u64),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryError::Json(msg) => write!(f, "not a library document: {msg}"),
            LibraryError::Schema(schema) => write!(
                f,
                "the library document is schema {schema}, which this ardana does not read; update ardana"
            ),
        }
    }
}

impl std::error::Error for LibraryError {}

impl LibraryDocument {
    /// Reads an index as the `ardana` version `version` (`MAJOR.MINOR.PATCH`) reads it: unknown fields are ignored, an
    /// entry that does not read or whose `min_version` is above `version` is left out, and another schema is refused.
    pub fn parse(text: &str, version: &str) -> Result<LibraryDocument, LibraryError> {
        #[derive(Deserialize)]
        struct Raw {
            schema: u32,
            default: String,
            #[serde(default)]
            browser_default: Option<String>,
            #[serde(default)]
            models: Vec<Value>,
        }
        let raw: Raw = read_schema(text)?;
        Ok(LibraryDocument {
            schema: raw.schema,
            default: raw.default,
            browser_default: raw.browser_default,
            models: raw
                .models
                .into_iter()
                .filter_map(|value| LibraryEntry::read(value, version))
                .collect(),
        })
    }
}

impl LibraryManifest {
    /// Reads a manifest as [`LibraryDocument::parse`] reads an index: `None` when the entry is one this version does
    /// not read.
    pub fn parse(text: &str, version: &str) -> Result<Option<LibraryEntry>, LibraryError> {
        #[derive(Deserialize)]
        struct Raw {
            model: Value,
        }
        let raw: Raw = read_schema(text)?;
        Ok(LibraryEntry::read(raw.model, version))
    }
}

/// `text` as a `T`, once its `schema` is the one this reader reads: the schema is checked before the shape, so a
/// later schema's document is refused for its schema, whatever else changed in it.
fn read_schema<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, LibraryError> {
    let json = |err: serde_json::Error| LibraryError::Json(err.to_string());
    let value: Value = serde_json::from_str(text).map_err(json)?;
    let schema = value
        .get("schema")
        .and_then(Value::as_u64)
        .ok_or_else(|| LibraryError::Json("it has no schema number".into()))?;
    if schema != u64::from(LIBRARY_SCHEMA) {
        return Err(LibraryError::Schema(schema));
    }
    serde_json::from_value(value).map_err(json)
}

impl LibraryEntry {
    /// The entry `value` holds, when it reads and `version` is at least its `min_version`.
    fn read(value: Value, version: &str) -> Option<LibraryEntry> {
        let entry: LibraryEntry = serde_json::from_value(value).ok()?;
        entry
            .min_version
            .as_deref()
            .is_none_or(|min| at_least(version, min))
            .then_some(entry)
    }
}

/// Whether `version` is at least `min`, both dotted numbers (a missing part is 0); false when either does not parse,
/// so an entry asking for a version this reader cannot compare is left out.
fn at_least(version: &str, min: &str) -> bool {
    let parts = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|p| p.parse().ok()).collect() };
    let (Some(mut version), Some(mut min)) = (parts(version), parts(min)) else {
        return false;
    };
    let len = version.len().max(min.len());
    version.resize(len, 0);
    min.resize(len, 0);
    version >= min
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, extra: &str) -> String {
        format!(
            r#"{{"name": "{name}", "kind": "decider", "summary": "S.", "base": "o/b", "params": "1B",
                "weights": "hf.co/o/{name}-GGUF", "commit": "{}", "quant": "Q4_K_M", "size": 1,
                "release_date": "2026-01-01", "tags": [{{"tag": "q4_k_m", "size": 1}}]{extra}}}"#,
            "a".repeat(40)
        )
    }

    /// R2.3: a schema 1 document with fields and values a later ardana added still reads, entries this version cannot
    /// read are left out, and another schema is refused with the advice to update.
    #[test]
    fn tolerates_newer_documents() {
        let version = "0.1.0";
        let text = format!(
            r#"{{"schema": 1, "default": "a", "browser_default": "a", "future": true, "models": [
                {},
                {},
                {},
                {},
                {},
                {}
            ]}}"#,
            entry(
                "a",
                r#", "layout": "plain", "later": {"x": 1}, "browser": {"weights": "hf.co/o/a-ONNX",
                   "commit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "quant": "int4", "size": 2,
                   "profile": {"name": "a"}, "extra": 1}"#
            ),
            entry("b", r#", "min_version": "0.1""#),
            entry("c", r#", "min_version": "0.2.0""#),
            entry("d", r#", "layout": "mixed""#),
            entry("e", r#", "min_version": "soon""#),
            r#"{"name": "f", "weights": "hf.co/o/f-GGUF"}"#,
        );
        let document = LibraryDocument::parse(&text, version).unwrap();
        assert_eq!(document.schema, 1);
        assert_eq!(document.default, "a");
        assert_eq!(document.browser_default.as_deref(), Some("a"));
        let names: Vec<&str> = document.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        let a = &document.models[0];
        assert_eq!(a.layout, Some(LayoutKind::Plain));
        assert_eq!(
            a.tags,
            [LibraryTag {
                tag: "q4_k_m".into(),
                size: 1
            }]
        );
        let browser = a.browser.as_ref().unwrap();
        assert_eq!(
            (
                browser.weights.as_str(),
                browser.quant.as_str(),
                browser.size
            ),
            ("hf.co/o/a-ONNX", "int4", 2)
        );
        assert_eq!(browser.profile["name"], "a");
        // A later version reads the entries it is new enough for.
        let later = LibraryDocument::parse(&text, "0.2.0").unwrap();
        let names: Vec<&str> = later.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);

        let manifest = format!(
            r#"{{"schema": 1, "model": {}, "future": 1}}"#,
            entry("g", "")
        );
        let g = LibraryManifest::parse(&manifest, version).unwrap().unwrap();
        assert_eq!((g.name.as_str(), g.size), ("g", 1));
        let skipped = format!(
            r#"{{"schema": 1, "model": {}}}"#,
            entry("h", r#", "min_version": "9.0.0""#)
        );
        assert_eq!(LibraryManifest::parse(&skipped, version).unwrap(), None);

        // A later schema is refused for its schema, whatever else it changed: the shape is read after that check.
        let later = text
            .replace("\"schema\": 1", "\"schema\": 2")
            .replace("\"default\"", "\"default_model\"");
        let err = LibraryDocument::parse(&later, version).unwrap_err();
        assert_eq!(err, LibraryError::Schema(2));
        assert!(err.to_string().contains("update ardana"), "{err}");
        let later = manifest
            .replace("\"schema\": 1", "\"schema\": 2")
            .replace("\"model\"", "\"entry\"");
        assert_eq!(
            LibraryManifest::parse(&later, version).unwrap_err(),
            LibraryError::Schema(2)
        );
        for text in [
            "{\"models\": []}",
            "{\"schema\": \"1\"}",
            "{\"schema\": 1}",
            "[]",
        ] {
            let err = LibraryDocument::parse(text, version).unwrap_err();
            assert!(matches!(err, LibraryError::Json(_)), "{text}: {err:?}");
        }
        let err = LibraryDocument::parse("not json", version).unwrap_err();
        assert!(
            err.to_string().starts_with("not a library document: "),
            "{err}"
        );

        assert!(at_least("0.1.0", "0.1"));
        assert!(at_least("1.0.0", "0.9.9"));
        assert!(at_least("0.1.0", "0.1.0"));
        assert!(!at_least("0.1.0", "0.1.1"));
        assert!(!at_least("0.1.0", "1"));
        assert!(!at_least("0.1.0", "v0.1.0"));
        assert!(!at_least("dev", "0.1.0"));
    }

    #[test]
    fn layouts_serialise_lowercase() {
        assert_eq!(
            serde_json::to_string(&LayoutKind::Chat).unwrap(),
            "\"chat\""
        );
        assert_eq!(
            serde_json::from_str::<LayoutKind>("\"plain\"").unwrap(),
            LayoutKind::Plain
        );
    }
}
