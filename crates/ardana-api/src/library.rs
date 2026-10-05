//! The model library document (C1): `https://ardana.ai/models.json`, the index of every library model family, and
//! `https://ardana.ai/models/<family>.json`, one family's manifest (C2). The `ardana` binary, the standalone playground
//! and xtask read it at run time; no copy is compiled into a binary.
//!
//! A family holds its sizes, each one checkpoint with its GGUF quants and at most one browser variant (Q1, Q5, Q6). The
//! document is `schema` 1 and changes additively (Q3): a reader ignores fields it does not know, leaves out a size or a
//! family it cannot read or whose `min_version` is above its own version, a size whose repositories are not
//! `hf.co/<org>/<repo>` at a 40-hex commit and a family whose name an earlier family took (Q13), and refuses another
//! `schema` value with an error that says to update `ardana`. A size reads with its family's `kind`, `summary`,
//! `license` and `layout` where it carries none (Q4). [`LibraryDocument::parse`] and [`LibraryManifest::parse`] read
//! that way, for every reader. [`LibrarySize::name`] is a quant's canonical name (Q7).

use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// The one schema this reader reads.
pub const LIBRARY_SCHEMA: u32 = 1;

/// The prefix of a Hugging Face repository in the document, `hf.co/<org>/<repo>`.
const HF_PREFIX: &str = "hf.co/";

/// The whole library: `GET <origin>/models.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryDocument {
    pub schema: u32,
    /// The model a request without a model uses when nothing is pulled, a canonical name (`decider:2b`).
    pub default: String,
    /// The size with a browser variant that a playground offers first, a canonical name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_default: Option<String>,
    #[serde(deserialize_with = "readable")]
    pub models: Vec<LibraryFamily>,
}

/// One family's manifest: `GET <origin>/models/<family>.json`, the same family object as the index holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryManifest {
    pub schema: u32,
    pub model: LibraryFamily,
}

/// One library model family, its sizes inside (Q1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryFamily {
    /// Lowercase letters, digits, `.`, `_` and `-`.
    pub name: String,
    /// `decider` or `instruct`.
    pub kind: String,
    /// One sentence for the pages.
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// The prompt layout; absent, a pull infers it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutKind>,
    /// The least `ardana` version that reads this family; older versions leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    /// The size the family's name alone and its tag `latest` stand for (Q2).
    pub latest: String,
    #[serde(deserialize_with = "readable")]
    pub sizes: Vec<LibrarySize>,
}

/// One size of a family: one checkpoint, its GGUF quants and its browser variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibrarySize {
    /// The size's tag, as `2b` or `26b-a4b`.
    pub size: String,
    /// The parameter count, as `1.9B`.
    pub params: String,
    /// The checkpoint the size is based on, `<org>/<repo>`.
    pub base: String,
    /// `YYYY-MM-DD`.
    pub release_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The size's own, else the family's once read (Q4); likewise `summary`, `license` and `layout`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutKind>,
    /// The least `ardana` version that reads this size; not inherited from the family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    /// The checkpoint the browser variant (and a GGUF ardana-ai hosts) is built from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RepoPin>,
    /// The tokenizer's repository, when the GGUF repository carries none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokenizer: Option<RepoPin>,
    pub gguf: GgufEntry,
    /// The browser variant, ONNX weights a visitor's tab runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserEntry>,
}

/// A Hugging Face repository at one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoPin {
    /// `hf.co/<org>/<repo>`.
    pub repo: String,
    /// The 40-hex commit the repository is read at.
    pub commit: String,
}

/// A size's GGUF repository and the quants it holds (Q6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GgufEntry {
    /// `hf.co/<org>/<repo>`.
    pub repo: String,
    /// The 40-hex commit of `repo` every quant is read at.
    pub commit: String,
    /// The quant `<family>:<size>` stands for, the first of `quants`.
    pub default: String,
    pub quants: Vec<GgufQuant>,
}

/// One quant of a size: the GGUF a pull downloads (Q14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GgufQuant {
    /// The quant's tag, as `q4_k_m`.
    pub quant: String,
    /// The GGUF's path in the repository.
    pub file: String,
    pub bytes: u64,
}

/// A size's ONNX weights for onnxruntime-web, built by `cargo xtask onnx convert`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserEntry {
    /// `hf.co/<org>/<repo>`, the ONNX repository.
    pub repo: String,
    /// The 40-hex commit of the files a tab downloads.
    pub commit: String,
    /// The weight quantization: `int4` or `int8`.
    pub quant: String,
    /// The bytes a browser downloads: `model.onnx`, `model.onnx.data` and `tokenizer.json`.
    pub bytes: u64,
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
    /// Reads an index as the `ardana` version `version` (`MAJOR.MINOR.PATCH`) reads it: unknown fields are ignored, a
    /// size or a family that does not read is left out ([`LibraryFamily::read`]), as is a family whose name an earlier
    /// family took, and another schema is refused.
    pub fn parse(text: &str, version: &str) -> Result<LibraryDocument, LibraryError> {
        let mut document: LibraryDocument = read_schema(text)?;
        let mut kept: Vec<LibraryFamily> = Vec::with_capacity(document.models.len());
        for family in std::mem::take(&mut document.models) {
            if let Some(family) = family.read(version)
                && !kept.iter().any(|f| f.name == family.name)
            {
                kept.push(family);
            }
        }
        document.models = kept;
        Ok(document)
    }
}

impl LibraryManifest {
    /// Reads a manifest as [`LibraryDocument::parse`] reads an index: `None` when the family is one this version does
    /// not read.
    pub fn parse(text: &str, version: &str) -> Result<Option<LibraryFamily>, LibraryError> {
        #[derive(Deserialize)]
        struct Raw {
            model: Value,
        }
        let raw: Raw = read_schema(text)?;
        let family: Option<LibraryFamily> = serde_json::from_value(raw.model).ok();
        Ok(family.and_then(|family| family.read(version)))
    }
}

/// `text` as a `T`, once its `schema` is the one this reader reads: the schema is checked before the shape, so a
/// later schema's document is refused for its schema, whatever else changed in it.
fn read_schema<T: DeserializeOwned>(text: &str) -> Result<T, LibraryError> {
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

/// The items of a list that read as `T`: one that does not is left out and its siblings stay (Q13).
fn readable<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let items = Vec::<Value>::deserialize(deserializer)?;
    Ok(items
        .into_iter()
        .filter_map(|item| serde_json::from_value(item).ok())
        .collect())
}

impl LibraryFamily {
    /// The family as the `ardana` version `version` reads it (Q4, Q13): `None` when its `min_version` is above
    /// `version` or no size is left once the sizes whose `min_version` is, or whose repositories are not
    /// [`LibrarySize::pinned`], are left out; each size with the family's `kind`, `summary`, `license` and `layout`
    /// where it carries none.
    fn read(mut self, version: &str) -> Option<LibraryFamily> {
        let readable =
            |min: &Option<String>| min.as_deref().is_none_or(|min| at_least(version, min));
        if !readable(&self.min_version) {
            return None;
        }
        self.sizes
            .retain(|size| readable(&size.min_version) && size.pinned());
        for size in &mut self.sizes {
            size.kind.get_or_insert_with(|| self.kind.clone());
            size.summary.get_or_insert_with(|| self.summary.clone());
            size.license = size.license.take().or_else(|| self.license.clone());
            size.layout = size.layout.or(self.layout);
        }
        (!self.sizes.is_empty()).then_some(self)
    }
}

impl LibrarySize {
    /// The quant `<family>:<size>` stands for, when `gguf.quants` lists it.
    pub fn default_quant(&self) -> Option<&GgufQuant> {
        self.gguf
            .quants
            .iter()
            .find(|quant| quant.quant == self.gguf.default)
    }

    /// The canonical tag of `quant` (Q7): `<size>` for the default quant, `<size>-<quant>` for another.
    pub fn tag(&self, quant: &GgufQuant) -> String {
        if quant.quant == self.gguf.default {
            self.size.clone()
        } else {
            format!("{}-{}", self.size, quant.quant)
        }
    }

    /// The canonical name of `quant` of this size of the family `family` (Q7): `<family>:<size>` for the default
    /// quant, `<family>:<size>-<quant>` for another.
    pub fn name(&self, family: &str, quant: &GgufQuant) -> String {
        format!("{family}:{}", self.tag(quant))
    }

    /// The reference a pull of `quant` reads (Q14): its file in the GGUF repository, `hf.co/<org>/<repo>:<file>`.
    pub fn reference(&self, quant: &GgufQuant) -> String {
        format!("{}:{}", self.gguf.repo, quant.file)
    }

    /// Whether every repository the size names is `hf.co/<org>/<repo>` at a 40-hex (lowercase) commit (Q13).
    fn pinned(&self) -> bool {
        let pins = [
            Some((&self.gguf.repo, &self.gguf.commit)),
            self.source.as_ref().map(|pin| (&pin.repo, &pin.commit)),
            self.tokenizer.as_ref().map(|pin| (&pin.repo, &pin.commit)),
            self.browser.as_ref().map(|b| (&b.repo, &b.commit)),
        ];
        pins.into_iter()
            .flatten()
            .all(|(repo, commit)| hf_repo(repo) && is_commit(commit))
    }
}

/// Whether `repo` is `hf.co/<org>/<repo>`, each part letters, digits, `.`, `_` and `-` (not `.` or `..`), without a
/// quant or file.
fn hf_repo(repo: &str) -> bool {
    let part = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    repo.strip_prefix(HF_PREFIX)
        .and_then(|path| path.split_once('/'))
        .is_some_and(|(org, repo)| part(org) && part(repo))
}

/// Whether `commit` is a 40-hex (lowercase) commit id.
fn is_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `version` is at least `min`, both dotted numbers (a missing part is 0); false when either does not parse,
/// so a family or size asking for a version this reader cannot compare is left out.
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

    /// A size `size` of one GGUF quant `q4_k_m`, with `extra` fields.
    fn size(size: &str, extra: &str) -> String {
        format!(
            r#"{{"size": "{size}", "params": "1B", "base": "o/b", "release_date": "2026-01-01",
                "gguf": {{"repo": "hf.co/o/r-GGUF", "commit": "{}", "default": "q4_k_m",
                          "quants": [{{"quant": "q4_k_m", "file": "r-{size}-Q4_K_M.gguf", "bytes": 1}}]}}{extra}}}"#,
            "a".repeat(40)
        )
    }

    /// A family `name` of `sizes`, with `extra` fields.
    fn family(name: &str, extra: &str, sizes: &[String]) -> String {
        format!(
            r#"{{"name": "{name}", "kind": "decider", "summary": "S.", "latest": "1b", "sizes": [{}]{extra}}}"#,
            sizes.join(", ")
        )
    }

    fn sizes(family: &LibraryFamily) -> Vec<&str> {
        family.sizes.iter().map(|s| s.size.as_str()).collect()
    }

    /// R2.1: a size reads with its family's `kind`, `summary`, `license` and `layout` where it carries none and keeps
    /// its own where it does; the pins, the quants and the browser variant read as written.
    #[test]
    fn reads_families_and_what_a_size_inherits() {
        let text = format!(
            r#"{{"schema": 1, "default": "a:1b", "browser_default": "a:1b", "models": [{}]}}"#,
            family(
                "a",
                r#", "license": "Apache-2.0", "layout": "chat""#,
                &[
                    size(
                        "1b",
                        &format!(
                            r#", "version": "v1", "source": {{"repo": "hf.co/o/s", "commit": "{}"}},
                               "tokenizer": {{"repo": "hf.co/o/t", "commit": "{}"}},
                               "browser": {{"repo": "hf.co/o/r-ONNX", "commit": "{}", "quant": "int4",
                                           "bytes": 2, "profile": {{"name": "a:1b"}}}}"#,
                            "c".repeat(40),
                            "d".repeat(40),
                            "b".repeat(40)
                        )
                    ),
                    size(
                        "2b",
                        r#", "kind": "instruct", "summary": "T.", "license": "MIT", "layout": "plain""#
                    ),
                ]
            )
        );
        let document = LibraryDocument::parse(&text, "0.1.0").unwrap();
        assert_eq!(
            (
                document.default.as_str(),
                document.browser_default.as_deref()
            ),
            ("a:1b", Some("a:1b"))
        );
        let [a] = &document.models[..] else {
            panic!("{document:?}")
        };
        assert_eq!(
            (a.name.as_str(), a.latest.as_str(), sizes(a)),
            ("a", "1b", vec!["1b", "2b"])
        );
        let (one, two) = (&a.sizes[0], &a.sizes[1]);
        let inherited = |s: &LibrarySize| {
            (
                s.kind.clone(),
                s.summary.clone(),
                s.license.clone(),
                s.layout,
            )
        };
        assert_eq!(
            inherited(one),
            (
                Some("decider".into()),
                Some("S.".into()),
                Some("Apache-2.0".into()),
                Some(LayoutKind::Chat)
            )
        );
        assert_eq!(
            inherited(two),
            (
                Some("instruct".into()),
                Some("T.".into()),
                Some("MIT".into()),
                Some(LayoutKind::Plain)
            )
        );
        assert_eq!(one.version.as_deref(), Some("v1"));
        assert_eq!(
            one.source,
            Some(RepoPin {
                repo: "hf.co/o/s".into(),
                commit: "c".repeat(40)
            })
        );
        assert_eq!(
            one.tokenizer.as_ref().map(|t| t.repo.as_str()),
            Some("hf.co/o/t")
        );
        assert_eq!(
            one.gguf,
            GgufEntry {
                repo: "hf.co/o/r-GGUF".into(),
                commit: "a".repeat(40),
                default: "q4_k_m".into(),
                quants: vec![GgufQuant {
                    quant: "q4_k_m".into(),
                    file: "r-1b-Q4_K_M.gguf".into(),
                    bytes: 1
                }],
            }
        );
        let browser = one.browser.as_ref().unwrap();
        assert_eq!(
            (browser.repo.as_str(), browser.quant.as_str(), browser.bytes),
            ("hf.co/o/r-ONNX", "int4", 2)
        );
        assert_eq!(browser.profile["name"], "a:1b");
        assert_eq!(two.source, None);
        assert_eq!(two.browser, None);

        // A family without a license or layout leaves a size without one.
        let text = format!(
            r#"{{"schema": 1, "default": "a", "models": [{}]}}"#,
            family("a", "", &[size("1b", "")])
        );
        let document = LibraryDocument::parse(&text, "0.1.0").unwrap();
        let one = &document.models[0].sizes[0];
        assert_eq!((one.license.as_deref(), one.layout), (None, None));
    }

    /// R2.1: a schema 1 document with fields and values a later ardana added still reads; each size and each family
    /// this version cannot read is left out, its siblings staying; another schema is refused with the advice to update.
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
                {},
                {}
            ]}}"#,
            family(
                "a",
                r#", "later": {"x": 1}"#,
                &[
                    size(
                        "1b",
                        &format!(
                            r#", "layout": "plain", "later": {{"x": 1}}, "browser": {{"repo": "hf.co/o/a-ONNX",
                               "commit": "{}", "quant": "int4", "bytes": 2, "profile": {{"name": "a"}}, "extra": 1}}"#,
                            "b".repeat(40)
                        )
                    ),
                    size("2b", r#", "min_version": "0.1""#),
                    size("3b", r#", "min_version": "0.2.0""#),
                    size("4b", r#", "layout": "mixed""#),
                    size("5b", r#", "min_version": "soon""#),
                    r#"{"size": "6b", "gguf": {"repo": "hf.co/o/r-GGUF"}}"#.to_string(),
                    size("7b", ""),
                ]
            ),
            family("b", r#", "min_version": "0.1""#, &[size("1b", "")]),
            family("c", r#", "min_version": "0.2.0""#, &[size("1b", "")]),
            family("d", r#", "layout": "mixed""#, &[size("1b", "")]),
            family("e", "", &[size("1b", r#", "min_version": "9""#)]),
            family("f", "", &[]),
            r#"{"name": "g", "sizes": []}"#,
        );
        let document = LibraryDocument::parse(&text, version).unwrap();
        assert_eq!(document.schema, 1);
        assert_eq!(document.default, "a");
        assert_eq!(document.browser_default.as_deref(), Some("a"));
        let names: Vec<&str> = document.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        let a = &document.models[0];
        assert_eq!(sizes(a), ["1b", "2b", "7b"]);
        assert_eq!(a.sizes[0].layout, Some(LayoutKind::Plain));
        let browser = a.sizes[0].browser.as_ref().unwrap();
        assert_eq!(
            (browser.repo.as_str(), browser.quant.as_str(), browser.bytes),
            ("hf.co/o/a-ONNX", "int4", 2)
        );
        assert_eq!(browser.profile["name"], "a");
        // A later version reads the families and sizes it is new enough for.
        let later = LibraryDocument::parse(&text, "0.2.0").unwrap();
        let names: Vec<&str> = later.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        assert_eq!(sizes(&later.models[0]), ["1b", "2b", "3b", "7b"]);

        let manifest = format!(
            r#"{{"schema": 1, "model": {}, "future": 1}}"#,
            family("h", "", &[size("1b", "")])
        );
        let h = LibraryManifest::parse(&manifest, version).unwrap().unwrap();
        assert_eq!((h.name.as_str(), sizes(&h)), ("h", vec!["1b"]));
        for skipped in [
            family("i", r#", "min_version": "9.0.0""#, &[size("1b", "")]),
            family("i", "", &[size("1b", r#", "min_version": "9.0.0""#)]),
            r#"{"name": "i"}"#.to_string(),
        ] {
            let skipped = format!(r#"{{"schema": 1, "model": {skipped}}}"#);
            assert_eq!(LibraryManifest::parse(&skipped, version).unwrap(), None);
        }

        // A later schema is refused for its schema, whatever else it changed: the shape is read after that check.
        let later = text
            .replace("\"schema\": 1", "\"schema\": 2")
            .replace("\"default\"", "\"default_model\"");
        let err = LibraryDocument::parse(&later, version).unwrap_err();
        assert_eq!(err, LibraryError::Schema(2));
        assert!(err.to_string().contains("update ardana"), "{err}");
        let later = manifest
            .replace("\"schema\": 1", "\"schema\": 2")
            .replace("\"model\"", "\"family\"");
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

    /// R2.1, Q13: a size whose repository is not `hf.co/<org>/<repo>` or whose commit is not 40 lowercase hex is left
    /// out and its siblings stay; a family with no size left, or whose name an earlier family took, is left out; the
    /// rest of the document stands.
    #[test]
    fn invalid_sizes_are_left_out() {
        let commit = "a".repeat(40);
        let read = |families: &[String]| {
            let text = format!(
                r#"{{"schema": 1, "default": "a:1b", "models": [{}]}}"#,
                families.join(", ")
            );
            LibraryDocument::parse(&text, "0.1.0").unwrap_or_else(|err| panic!("{err}:\n{text}"))
        };
        let names = |document: &LibraryDocument| -> Vec<String> {
            document
                .models
                .iter()
                .flat_map(|family| {
                    family
                        .sizes
                        .iter()
                        .map(|size| size.name(&family.name, size.default_quant().unwrap()))
                })
                .collect()
        };
        let browser = |commit: &str| {
            format!(
                r#", "browser": {{"repo": "hf.co/o/r-ONNX", "commit": "{commit}", "quant": "int4", "bytes": 1,
                      "profile": {{}}}}"#
            )
        };
        let ok = read(&[family("a", "", &[size("1b", &browser(&commit))])]);
        assert_eq!(names(&ok), ["a:1b"]);
        assert_eq!(ok.default, "a:1b");

        let pin = |field: &str, repo: &str, commit: &str| {
            format!(r#", "{field}": {{"repo": "{repo}", "commit": "{commit}"}}"#)
        };
        for bad in [
            size("2b", &pin("tokenizer", "ollama:x", &commit)),
            size("2b", &pin("tokenizer", "hf.co/o/t", "main")),
            size("2b", &pin("source", "hf.co/o/s:q4_0", &commit)),
            size("2b", &pin("source", "hf.co/o/s", &commit.to_uppercase())),
            size("2b", "").replace("hf.co/o/r-GGUF", "o/r-GGUF"),
            size("2b", "").replace("hf.co/o/r-GGUF", "hf.co/o/r-GGUF:Q4_0"),
            size("2b", "").replace(&commit, &commit[..7]),
            size("2b", &browser("c")),
            size("2b", &browser(&commit)).replace("hf.co/o/r-ONNX", "hf.co/o/r-ONNX:int4"),
        ] {
            let document = read(&[family("a", "", &[size("1b", ""), bad.clone()])]);
            assert_eq!(names(&document), ["a:1b"], "{bad}");
            // A family whose one size is left out is left out, and the document's other fields read the same.
            let document = read(&[
                family("a", "", std::slice::from_ref(&bad)),
                family("b", "", &[size("1b", "")]),
            ]);
            assert_eq!(names(&document), ["b:1b"], "{bad}");
            assert_eq!(document.default, "a:1b");
            // A manifest's one family reads the same way.
            let manifest = format!(
                r#"{{"schema": 1, "model": {}}}"#,
                family("a", "", std::slice::from_ref(&bad))
            );
            assert_eq!(LibraryManifest::parse(&manifest, "0.1.0").unwrap(), None);
        }
        // The first family of a name is the one read.
        let document = read(&[
            family("a", "", &[size("1b", "")]),
            family("a", "", &[size("2b", "")]),
        ]);
        assert_eq!(names(&document), ["a:1b"]);
        // A reader checks nothing else: a browser quant it does not know and an odd quant name read.
        let document = read(&[family(
            "a",
            "",
            &[size("1b", &browser(&commit))
                .replace("\"int4\"", "\"fp8\"")
                .replace("\"q4_k_m\"", "\"Q4 0\"")],
        )]);
        assert_eq!(document.models.len(), 1);
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
