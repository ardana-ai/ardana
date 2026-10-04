//! The model library: short names such as `decider-2b` for Hugging Face GGUF repositories, which [`crate::pull`]
//! resolves like the `hf.co/` reference they stand for; `<name>:<quant>` picks another GGUF of the same repository,
//! matched case-insensitively like an `hf.co/` quant, and a model may have a browser variant, ONNX weights
//! onnxruntime-web runs in a visitor's tab ([`BrowserEntry`]).
//!
//! The library is one published document ([`LibraryDocument`], `https://ardana.ai/models.json`, with one manifest per
//! model beside it at `models/<name>.json`), read at run time: no model list is compiled in, and [`DEFAULT_MODEL`] is
//! the one name the binary keeps (Q1). [`LibrarySource`] is what `ARDANA_LIBRARY` names (C4); [`LibraryClient`] reads
//! the library from it through the cache in the Ardana home (`$ARDANA_HOME/library`, the bytes as fetched, Q7): `pull`
//! and `run` of a model not pulled GET its manifest every time and fall back to the cached copy, while `list`, `show`,
//! `rm`, `ps` and help read the cache alone (Q8); `serve` starts from the cached index and GETs the index once, then
//! once every refresh, with `If-None-Match` on the index's `ETag`, keeping the cached copy when a GET fails (Q9), and
//! asks for the manifest of a name its index lacks (Q10). Every GET carries `User-Agent: ardana/<version>` and nothing
//! else that identifies the machine (Q17). [`Library`] is a document as read: the entries this version reads, an
//! invalid one left out (Q12).

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use ardana_core::{
    BrowserEntry, LibraryDocument, LibraryEntry, LibraryError, LibraryManifest, LibraryTag,
};

use crate::refs::{HF_PREFIX, HfFile, OLLAMA_PREFIX, Ref, valid_name};
use crate::{Registry, RegistryError, write_atomically};

/// The variable naming the library's source (C4): unset, the published library; an `http(s)://…/models.json` URL,
/// that index with its manifests beside it; a file path, that one document and no network; `off`, no library.
pub const LIBRARY_VAR: &str = "ARDANA_LIBRARY";
/// The published library, the source when [`LIBRARY_VAR`] is unset.
pub const LIBRARY_URL: &str = "https://ardana.ai/models.json";
/// The [`LIBRARY_VAR`] value that turns the library off.
pub const LIBRARY_OFF: &str = "off";
/// Where people read about the library's models; help and error texts point here and name no model (Q11).
pub const MODELS_PAGE: &str = "https://ardana.ai/models/";
/// The model requests without a model use while nothing is pulled and no document is at hand: the one model name
/// the binary keeps (Q1).
pub const DEFAULT_MODEL: &str = "decider-2b";
/// The `User-Agent` of every library GET, and all a GET says about this machine (Q17).
pub const USER_AGENT: &str = concat!("ardana/", env!("CARGO_PKG_VERSION"));
/// The directory of the library's cache inside the Ardana home (Q7), holding the index as `models.json` and each
/// manifest as `models/<name>.json`, the bytes as fetched.
pub const CACHE_DIR: &str = "library";
/// The cached index's file name, and the published index's.
pub const INDEX_FILE: &str = "models.json";
/// The file beside the cached index holding its `ETag`, which the next index GET sends as `If-None-Match` (Q7).
pub const ETAG_FILE: &str = "models.json.etag";
/// The directory of the manifests, in the cache and beside the published index alike.
pub const MANIFEST_DIR: &str = "models";

/// The version of this `ardana`, which a document's `min_version` is compared with (Q12).
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The quantizations `cargo xtask onnx convert` builds browser variants in.
const BROWSER_QUANTS: [&str; 2] = ["int4", "int8"];
/// How long a library GET may take to connect, and in all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TIMEOUT: Duration = Duration::from_secs(60);

/// Where the library comes from (C4): what [`LIBRARY_VAR`] names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibrarySource {
    /// The index at this URL, with the manifests at `models/<name>.json` beside it.
    Url(String),
    /// This one document, with no network.
    File(PathBuf),
    /// No library: a library name is an unknown model.
    Off,
}

impl LibrarySource {
    /// What [`LIBRARY_VAR`] names in this process's environment.
    pub fn from_env() -> LibrarySource {
        LibrarySource::parse(std::env::var_os(LIBRARY_VAR).as_deref())
    }

    /// What a [`LIBRARY_VAR`] value names: unset or empty is [`LIBRARY_URL`], `off` (in any case) is no library, an
    /// `http://` or `https://` URL is an index, anything else a file path.
    pub fn parse(value: Option<&OsStr>) -> LibrarySource {
        let Some(value) = value.filter(|v| !v.is_empty()) else {
            return LibrarySource::Url(LIBRARY_URL.to_string());
        };
        let text = value.to_string_lossy();
        if text.trim().eq_ignore_ascii_case(LIBRARY_OFF) {
            LibrarySource::Off
        } else if text.starts_with("http://") || text.starts_with("https://") {
            LibrarySource::Url(text.trim().to_string())
        } else {
            LibrarySource::File(PathBuf::from(value))
        }
    }

    /// The URL of the manifest of the model `name`, beside the index (C2); `None` but for a URL source.
    pub fn manifest_url(&self, name: &str) -> Option<String> {
        let LibrarySource::Url(index) = self else {
            return None;
        };
        let base = index.rsplit_once('/').map_or("", |(base, _)| base);
        Some(format!(
            "{base}/{MANIFEST_DIR}/{}.json",
            name.to_ascii_lowercase()
        ))
    }
}

impl fmt::Display for LibrarySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibrarySource::Url(url) => f.write_str(url),
            LibrarySource::File(path) => write!(f, "{}", path.display()),
            LibrarySource::Off => f.write_str(LIBRARY_OFF),
        }
    }
}

/// The library as one process reads it: from its source (C4), through the cache in the Ardana home (Q7).
#[derive(Debug, Clone)]
pub struct LibraryClient {
    source: LibrarySource,
    /// `$ARDANA_HOME/library`.
    cache: PathBuf,
}

impl LibraryClient {
    /// The source [`LIBRARY_VAR`] names, cached in [`Registry::home`].
    pub fn from_env() -> Result<LibraryClient, RegistryError> {
        Ok(LibraryClient::new(
            LibrarySource::from_env(),
            &Registry::home()?,
        ))
    }

    /// `source`, cached under the Ardana home `home`.
    pub fn new(source: LibrarySource, home: &Path) -> LibraryClient {
        LibraryClient {
            source,
            cache: home.join(CACHE_DIR),
        }
    }

    pub fn source(&self) -> &LibrarySource {
        &self.source
    }

    /// The cached manifest of the model `name`.
    fn manifest_path(&self, name: &str) -> PathBuf {
        self.cache
            .join(MANIFEST_DIR)
            .join(format!("{}.json", name.to_ascii_lowercase()))
    }

    /// The library as `pull` and `run` read it for `input`, a `<name>[:<quant>]` (Q8): the whole document of a file
    /// source, nothing under `off`, and under a URL source the model's manifest, fetched every time (its body kept in
    /// the cache) with the cached copy standing in when the GET fails; a 404 is no library model, and an input that is
    /// no library name sends nothing. The library's entry for `input`, when it has one.
    pub async fn lookup(&self, input: &str) -> Result<Library, RegistryError> {
        let LibrarySource::Url(_) = &self.source else {
            return self.cached(input);
        };
        let Some(name) = library_name(input) else {
            return Ok(Library::empty());
        };
        let url = self
            .source
            .manifest_url(name)
            .expect("a URL source has manifest URLs");
        let path = self.manifest_path(name);
        let library_err = |msg: String| RegistryError::Library {
            url: url.clone(),
            msg,
        };
        let (body, fetched) = match fetch(&url, None).await {
            Ok(Fetched::Body { body, .. }) => (body, true),
            Ok(Fetched::NotFound) => return Ok(Library::empty()),
            Ok(Fetched::NotModified) => {
                return Err(library_err("answered 304 to a plain GET".into()));
            }
            Err(cause) => match std::fs::read(&path) {
                Ok(cached) => (cached, false),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    return Err(library_err(cause));
                }
                Err(err) => return Err(RegistryError::Io { path, err }),
            },
        };
        let text = std::str::from_utf8(&body).map_err(|err| library_err(err.to_string()))?;
        let entry =
            LibraryManifest::parse(text, VERSION).map_err(|err| library_err(err.to_string()))?;
        // The bytes as fetched are kept once they read, so a body that does not never replaces a good copy.
        if fetched {
            write_atomically(&path, &body)?;
        }
        Ok(entry.map_or_else(Library::empty, Library::single))
    }

    /// The library as the commands that send nothing read it for `input` (Q8: `list`, `show`, `rm`, `ps` and help):
    /// the whole document of a file source, nothing under `off`, and under a URL source the model's cached manifest,
    /// when a pull or run fetched one.
    pub fn cached(&self, input: &str) -> Result<Library, RegistryError> {
        match &self.source {
            LibrarySource::Off => Ok(Library::empty()),
            LibrarySource::File(path) => Library::from_file(path),
            LibrarySource::Url(_) => {
                let Some(name) = library_name(input) else {
                    return Ok(Library::empty());
                };
                let path = self.manifest_path(name);
                let text = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Library::empty());
                    }
                    Err(err) => return Err(RegistryError::Io { path, err }),
                };
                let entry = LibraryManifest::parse(&text, VERSION).map_err(|err| {
                    RegistryError::Invalid {
                        what: path.display().to_string(),
                        msg: err.to_string(),
                    }
                })?;
                Ok(entry.map_or_else(Library::empty, Library::single))
            }
        }
    }

    /// The whole library as `ardana serve` reads it when it starts, without the network: the document of a file
    /// source; under a URL source the index as last fetched (`$ARDANA_HOME/library/models.json`), or no model until
    /// [`LibraryClient::refresh`] fetches one, so the server lists the pulled models alone; nothing under `off`.
    pub fn index(&self) -> Result<Library, RegistryError> {
        match &self.source {
            LibrarySource::File(path) => Library::from_file(path),
            LibrarySource::Url(_) => {
                let path = self.index_path();
                if !path.exists() {
                    return Ok(Library::empty());
                }
                Library::from_file(&path)
            }
            LibrarySource::Off => Ok(Library::empty()),
        }
    }

    /// The whole library read again, as `ardana serve` reads it on start and every refresh (Q9): a file source's
    /// document; under a URL source one index GET, sent with `If-None-Match` on the cached index's `ETag`, whose 200
    /// is kept in the cache (the bytes as fetched, its `ETag` beside them) and read, and whose 304 (or a document this
    /// version does not read) keeps the cached copy. `None` when nothing new was read; an error naming the URL when
    /// the GET failed, the caller keeping the library it has.
    pub async fn refresh(&self) -> Result<Option<Library>, RegistryError> {
        let url = match &self.source {
            LibrarySource::Off => return Ok(None),
            LibrarySource::File(path) => return Library::from_file(path).map(Some),
            LibrarySource::Url(url) => url,
        };
        let library_err = |msg: String| RegistryError::Library {
            url: url.clone(),
            msg,
        };
        // An `ETag` stands for the cached index: without the index, nothing to revalidate.
        let etag_path = self.cache.join(ETAG_FILE);
        let known = match std::fs::read_to_string(&etag_path) {
            Ok(etag) if self.index_path().exists() => Some(etag),
            Ok(_) => None,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(RegistryError::Io {
                    path: etag_path,
                    err,
                });
            }
        };
        let (body, etag) = match fetch(url, known.as_deref()).await.map_err(library_err)? {
            Fetched::Body { body, etag } => (body, etag),
            Fetched::NotModified => return Ok(None),
            Fetched::NotFound => return Err(library_err("answered 404 Not Found".into())),
        };
        let text = String::from_utf8(body).map_err(|err| library_err(err.to_string()))?;
        let document =
            LibraryDocument::parse(&text, VERSION).map_err(|err| library_err(err.to_string()))?;
        write_atomically(&self.index_path(), text.as_bytes())?;
        match etag {
            Some(etag) => write_atomically(&etag_path, etag.as_bytes())?,
            None => match std::fs::remove_file(&etag_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(RegistryError::Io {
                        path: etag_path,
                        err,
                    });
                }
            },
        }
        Ok(Some(Library::from_document(document)))
    }

    /// The cached index.
    fn index_path(&self) -> PathBuf {
        self.cache.join(INDEX_FILE)
    }
}

/// What a GET answered.
enum Fetched {
    /// 200, with its body and its `ETag`.
    Body { body: Vec<u8>, etag: Option<String> },
    /// 304: what `If-None-Match` named is current.
    NotModified,
    /// 404: nothing is published there.
    NotFound,
}

/// `GET url` with [`USER_AGENT`], and `If-None-Match: if_none_match` when given; any other failure, a status that is
/// none of 200, 304 and 404 included, as its cause.
async fn fetch(url: &str, if_none_match: Option<&str>) -> Result<Fetched, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TIMEOUT)
        .build()
        .map_err(|err| causes(&err))?;
    let mut request = client.get(url);
    if let Some(etag) = if_none_match {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await.map_err(|err| causes(&err))?;
    match response.status() {
        reqwest::StatusCode::OK => {
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let body = response.bytes().await.map_err(|err| causes(&err))?;
            Ok(Fetched::Body {
                body: body.to_vec(),
                etag,
            })
        }
        reqwest::StatusCode::NOT_MODIFIED => Ok(Fetched::NotModified),
        reqwest::StatusCode::NOT_FOUND => Ok(Fetched::NotFound),
        status => Err(format!("answered {status}")),
    }
}

/// `err` and every error under it, as `a: b: c`: a transport error names the connection's refusal, not the request.
fn causes(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        let cause = err.to_string();
        if !text.contains(&cause) {
            text.push_str(": ");
            text.push_str(&cause);
        }
        source = err.source();
    }
    text
}

/// The library name `input` names (`<name>[:<quant>]`, both in the name grammar), or `None` for anything else: a
/// reference (`hf.co/`, `ollama:`), a path (a `.gguf` file name included), or an input no library could hold.
pub fn library_name(input: &str) -> Option<&str> {
    if input.contains('/')
        || input.starts_with(OLLAMA_PREFIX)
        || input.to_ascii_lowercase().ends_with(".gguf")
    {
        return None;
    }
    let (name, quant) = match input.split_once(':') {
        Some((name, quant)) => (name, Some(quant)),
        None => (input, None),
    };
    (valid_name(name) && quant.is_none_or(valid_name)).then_some(name)
}

/// A library document as read: the models this `ardana` reads, the model requests use while nothing is pulled and
/// the first browser model.
#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// The model a request without a model uses when the registry is empty.
    pub default: String,
    /// The library model with a browser variant that a playground offers first.
    pub browser_default: Option<String>,
    pub models: Vec<LibraryEntry>,
}

/// A library model and the quant a name picks from it.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryPick {
    pub model: LibraryEntry,
    /// A quant other than the model's own, lowercased.
    quant: Option<String>,
}

impl LibraryPick {
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

impl Library {
    /// No model: the library of `off`, of a URL source before its index is fetched, and of a name no manifest answers
    /// for. Its default is [`DEFAULT_MODEL`].
    pub fn empty() -> Library {
        Library {
            default: DEFAULT_MODEL.to_string(),
            browser_default: None,
            models: Vec::new(),
        }
    }

    /// The one model a manifest holds, checked as [`Library::from_document`] checks an index's entries (Q12): an
    /// entry no pull reads is no library model.
    fn single(entry: LibraryEntry) -> Library {
        let mut library = Library::empty();
        if check_entry(&entry).is_ok() {
            library.models = vec![entry];
        }
        library
    }

    /// The document at `path`, read as [`Library::from_document`] reads it.
    pub fn from_file(path: &Path) -> Result<Library, RegistryError> {
        let text = std::fs::read_to_string(path).map_err(|err| RegistryError::Io {
            path: path.to_path_buf(),
            err,
        })?;
        let document =
            LibraryDocument::parse(&text, VERSION).map_err(|err| RegistryError::Invalid {
                what: path.display().to_string(),
                msg: err.to_string(),
            })?;
        Ok(Library::from_document(document))
    }

    /// `document`'s models this registry can pull (Q12): an entry whose name, repositories, commits, quants or
    /// source are not what a pull reads, or whose name an earlier entry took, is left out.
    pub fn from_document(document: LibraryDocument) -> Library {
        let mut models: Vec<LibraryEntry> = Vec::with_capacity(document.models.len());
        for entry in document.models {
            if check_entry(&entry).is_ok() && !models.iter().any(|m| m.name == entry.name) {
                models.push(entry);
            }
        }
        Library {
            default: document.default,
            browser_default: document.browser_default,
            models,
        }
    }

    /// The model names, in library order.
    pub fn names(&self) -> Vec<&str> {
        self.models.iter().map(|m| m.name.as_str()).collect()
    }

    /// The library model named exactly `name` and its browser variant, when it has one.
    pub fn browser(&self, name: &str) -> Option<(&LibraryEntry, &BrowserEntry)> {
        let model = self.models.iter().find(|m| m.name == name)?;
        Some((model, model.browser.as_ref()?))
    }

    /// The bytes a tab downloads to run the browser variant of the model `name`, when it has one.
    pub fn browser_size(&self, name: &str) -> Option<u64> {
        self.browser(name).map(|(_, browser)| browser.size)
    }

    /// Whether `name` is the browser default, the browser model a playground offers first.
    pub fn is_browser_default(&self, name: &str) -> bool {
        self.browser_default.as_deref() == Some(name)
    }

    /// The library model `input` names, `<name>` or `<name>:<quant>`, both matched case-insensitively; the model's
    /// own quant is the bare name. `None` for anything else.
    pub fn find(&self, input: &str) -> Option<LibraryPick> {
        let name = library_name(input)?;
        let model = self
            .models
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))?;
        let quant = match input.split_once(':') {
            None => None,
            Some((_, quant)) if quant.eq_ignore_ascii_case(&model.quant) => None,
            Some((_, quant)) => Some(quant.to_lowercase()),
        };
        Some(LibraryPick {
            model: model.clone(),
            quant,
        })
    }
}

/// Why `entry` is not one a pull reads, if it is not: the checks a document passes before any download.
fn check_entry(entry: &LibraryEntry) -> Result<(), String> {
    if !valid_name(&entry.name) || entry.name != entry.name.to_lowercase() {
        return Err("a name is lowercase letters, digits, '.', '_' or '-'".into());
    }
    let browser = entry.browser.as_ref();
    for repo in std::iter::once(&entry.weights)
        .chain(&entry.tokenizer)
        .chain(browser.map(|b| &b.weights))
    {
        if !hf_repo(repo) {
            return Err(format!("{repo:?} is not {HF_PREFIX}<org>/<repo>"));
        }
    }
    for commit in std::iter::once(&entry.commit)
        .chain(&entry.tokenizer_commit)
        .chain(browser.map(|b| &b.commit))
    {
        if !is_commit(commit) {
            return Err(format!("{commit:?} is not a 40-hex commit"));
        }
    }
    if entry.tokenizer.is_some() != entry.tokenizer_commit.is_some() {
        return Err("a tokenizer repository and its commit come together".into());
    }
    if let Some(source) = &entry.source
        && !source
            .split_once('@')
            .is_some_and(|(repo, commit)| hf_repo(repo) && is_commit(commit))
    {
        return Err(format!(
            "the source {source:?} is not {HF_PREFIX}<org>/<repo>@<commit>"
        ));
    }
    if let Some(browser) = browser
        && !BROWSER_QUANTS.contains(&browser.quant.as_str())
    {
        return Err(format!(
            "the browser quant {:?} is not one of {}",
            browser.quant,
            BROWSER_QUANTS.join(", ")
        ));
    }
    let quant = Ref::parse(&format!("{}:{}", entry.weights, entry.quant));
    if !matches!(
        quant,
        Ok(Ref::Hf {
            file: Some(HfFile::Quant(_)),
            ..
        })
    ) {
        return Err("the quant is not a quant name".into());
    }
    Ok(())
}

/// Whether `repo` is `hf.co/<org>/<repo>`, without a quant or file.
fn hf_repo(repo: &str) -> bool {
    repo.starts_with(HF_PREFIX) && matches!(Ref::parse(repo), Ok(Ref::Hf { file: None, .. }))
}

/// Whether `commit` is a 40-hex (lowercase) commit id.
fn is_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "a0a01d6f8135298f400a8c856b355793012ae971";

    /// A document of one model `a` with `fields` added to its entry, `browser` as its browser table (when given) and
    /// `head` replacing the top-level fields.
    fn document(head: &str, fields: &str, browser: Option<&str>) -> String {
        let browser = browser.map_or(String::new(), |b| {
            format!(
                r#", "browser": {{"weights": "hf.co/o/r-ONNX", "commit": "{COMMIT}", "quant": "int4", "size": 1,
                      "profile": {{}}{b}}}"#
            )
        });
        format!(
            r#"{{"schema": 1, {head}, "models": [{{"name": "a", "kind": "decider", "summary": "S.", "base": "o/b",
                "params": "1B", "weights": "hf.co/o/r", "commit": "{COMMIT}", "quant": "Q4_0", "size": 1,
                "release_date": "2026-01-01", "tags": []{fields}{browser}}}]}}"#
        )
    }

    fn read(text: &str) -> Library {
        let document =
            LibraryDocument::parse(text, VERSION).unwrap_or_else(|err| panic!("{err}:\n{text}"));
        Library::from_document(document)
    }

    #[test]
    fn sources_follow_the_variable() {
        let parse = |value: Option<&str>| LibrarySource::parse(value.map(OsStr::new));
        assert_eq!(parse(None), LibrarySource::Url(LIBRARY_URL.into()));
        assert_eq!(parse(Some("")), LibrarySource::Url(LIBRARY_URL.into()));
        assert_eq!(parse(Some("off")), LibrarySource::Off);
        assert_eq!(parse(Some("OFF")), LibrarySource::Off);
        assert_eq!(
            parse(Some("http://127.0.0.1:1/models.json")),
            LibrarySource::Url("http://127.0.0.1:1/models.json".into())
        );
        assert_eq!(
            parse(Some("tests/data/models.json")),
            LibrarySource::File("tests/data/models.json".into())
        );
        assert_eq!(
            parse(Some("https://ardana.ai/models.json")).manifest_url("Decider-2B"),
            Some("https://ardana.ai/models/decider-2b.json".into())
        );
        assert_eq!(LibrarySource::Off.manifest_url("decider-2b"), None);
        assert_eq!(LibrarySource::Off.to_string(), "off");
    }

    #[test]
    fn library_names_follow_the_grammar() {
        assert_eq!(library_name("decider-2b"), Some("decider-2b"));
        assert_eq!(library_name("Decider-2B:Q8_0"), Some("Decider-2B"));
        for other in [
            "",
            "decider-2b:",
            ":q8",
            "decider-2b:Q4 K",
            "decider-2b:a:b",
            "hf.co/Mapika/decider-2b-GGUF",
            "ollama:decider-2b",
            "./decider-2b",
            " decider-2b",
        ] {
            assert_eq!(library_name(other), None, "{other:?}");
        }
    }

    /// Q12 on the registry's side: an entry a pull could not read is left out, and the rest of the document stands.
    #[test]
    fn invalid_entries_are_left_out() {
        let ok = document(
            "\"default\": \"a\", \"browser_default\": \"a\"",
            "",
            Some(""),
        );
        let library = read(&ok);
        assert_eq!(library.names(), ["a"]);
        assert_eq!(library.default, "a");
        assert!(library.browser("a").is_some());
        assert_eq!(library.browser_size("a"), Some(1));
        assert!(library.is_browser_default("a"));
        let pick = library.find("A:q8_0").unwrap();
        assert_eq!(pick.name(), "a:q8_0");
        assert_eq!(pick.reference(), "hf.co/o/r:q8_0");
        assert_eq!(pick.size(), None);
        assert_eq!(library.find("a").unwrap().size(), Some(1));

        let head = "\"default\": \"a\"";
        for bad in [
            document(
                head,
                r#", "tokenizer": "ollama:x", "tokenizer_commit": "{COMMIT}""#,
                None,
            )
            .replace("{COMMIT}", COMMIT),
            document(head, r#", "tokenizer": "hf.co/o/t""#, None),
            document(head, r#", "source": "hf.co/o/s:1234""#, None),
            document(head, "", None).replace("hf.co/o/r", "o/r"),
            document(head, "", None).replace("hf.co/o/r", "hf.co/o/r:Q4_0"),
            document(head, "", None).replace("\"Q4_0\"", "\"Q4 0\""),
            document(head, "", None).replace("\"name\": \"a\"", "\"name\": \"A\""),
            document(head, "", None).replace(COMMIT, &COMMIT[..7]),
            document(head, "", Some("")).replace(COMMIT, &COMMIT.to_uppercase()),
            document(head, "", Some("")).replace("\"int4\"", "\"Q4_0\""),
            document(head, "", Some("")).replace("hf.co/o/r-ONNX", "hf.co/o/r-ONNX:int4"),
        ] {
            let library = read(&bad);
            assert_eq!(library.names(), [] as [&str; 0], "{bad}");
            // The document's other fields are read all the same.
            assert_eq!(library.default, "a");
        }
        let source = document(
            head,
            &format!(r#", "source": "hf.co/o/s@{COMMIT}""#),
            Some(""),
        );
        assert_eq!(read(&source).names(), ["a"]);
    }

    #[test]
    fn causes_name_the_chain() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert_eq!(causes(&io), "refused");
        let err = LibraryError::Schema(2);
        assert!(causes(&err).contains("update ardana"));
    }
}
