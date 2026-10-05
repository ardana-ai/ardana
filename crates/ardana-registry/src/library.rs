//! The model library: families of Hugging Face GGUF repositories with their sizes inside (Q1), which [`crate::pull`]
//! resolves by the file a size's quant names (Q14). A library name is `<family>[:<tag>]`; a family's tags are
//! `latest`, `<size>` and `<size>-<quant>` for every size and quant it holds, enumerated and never parsed (Q9), and a
//! size may have a browser variant, ONNX weights onnxruntime-web runs in a visitor's tab ([`BrowserEntry`]).
//! [`LibraryPick::name`] is the canonical name every reader shows (Q7): `<family>:<size>` for a size's default quant,
//! `<family>:<size>-<quant>` for another.
//!
//! The library is one published document ([`LibraryDocument`], `https://ardana.ai/models.json`, with one manifest per
//! family beside it at `models/<family>.json`, Q16), read at run time: no model list is compiled in, and
//! [`DEFAULT_MODEL`] is the one name the binary keeps (Q15). [`LibrarySource`] is what `ARDANA_LIBRARY` names;
//! [`LibraryClient`] reads the library from it through the cache in the Ardana home (`$ARDANA_HOME/library`, the bytes
//! as fetched): `pull` and `run` of a model not pulled GET its family's manifest every time and fall back to the cached
//! copy, while `list`, `show`, `rm`, `ps` and help read the cache alone; `serve` starts from the cached index and GETs
//! the index once, then once every refresh, with `If-None-Match` on the index's `ETag`, keeping the cached copy when a
//! GET fails, and asks for the manifest of a name its index lacks. Every GET carries `User-Agent: ardana/<version>` and
//! nothing else that identifies the machine. [`Library`] is a document as read: the families and sizes this version
//! reads, an invalid one left out by [`LibraryDocument::parse`] (Q13).

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use ardana_core::{
    BrowserEntry, GgufEntry, GgufQuant, LibraryDocument, LibraryError, LibraryFamily,
    LibraryManifest, LibrarySize, RepoPin,
};

use crate::refs::{OLLAMA_PREFIX, valid_name};
use crate::{Registry, RegistryError, write_atomically};

/// The variable naming the library's source: unset, the published library; an `http(s)://…/models.json` URL,
/// that index with its manifests beside it; a file path, that one document and no network; `off`, no library.
pub const LIBRARY_VAR: &str = "ARDANA_LIBRARY";
/// The published library, the source when [`LIBRARY_VAR`] is unset.
pub const LIBRARY_URL: &str = "https://ardana.ai/models.json";
/// The [`LIBRARY_VAR`] value that turns the library off.
pub const LIBRARY_OFF: &str = "off";
/// Where people read about the library's models; help and error texts point here and name no model but the default
/// one (Q12).
pub const MODELS_PAGE: &str = "https://ardana.ai/models/";
/// The model requests without a model use while nothing is pulled and no document is at hand: the family `decider`,
/// the one library name the binary keeps (Q15).
pub const DEFAULT_MODEL: &str = "decider";
/// The tag a family's name alone stands for (Q9).
pub const LATEST: &str = "latest";
/// The `User-Agent` of every library GET, and all a GET says about this machine.
pub const USER_AGENT: &str = concat!("ardana/", env!("CARGO_PKG_VERSION"));
/// The directory of the library's cache inside the Ardana home, holding the index as `models.json` and each family's
/// manifest as `models/<family>.json`, the bytes as fetched.
pub const CACHE_DIR: &str = "library";
/// The cached index's file name, and the published index's.
pub const INDEX_FILE: &str = "models.json";
/// The file beside the cached index holding its `ETag`, which the next index GET sends as `If-None-Match`.
pub const ETAG_FILE: &str = "models.json.etag";
/// The directory of the manifests, in the cache and beside the published index alike.
pub const MANIFEST_DIR: &str = "models";

/// The version of this `ardana`, which a document's `min_version` is compared with (Q13).
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long a library GET may take to connect, and in all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TIMEOUT: Duration = Duration::from_secs(60);

/// Where the library comes from: what [`LIBRARY_VAR`] names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibrarySource {
    /// The index at this URL, with the manifests at `models/<family>.json` beside it.
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

    /// The URL of the manifest of the family `family`, beside the index (C2, Q16); `None` but for a URL source.
    pub fn manifest_url(&self, family: &str) -> Option<String> {
        let LibrarySource::Url(index) = self else {
            return None;
        };
        let base = index.rsplit_once('/').map_or("", |(base, _)| base);
        Some(format!(
            "{base}/{MANIFEST_DIR}/{}.json",
            family.to_ascii_lowercase()
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

/// The library as one process reads it: from its source, through the cache in the Ardana home.
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

    /// The cached manifest of the family `family`.
    fn manifest_path(&self, family: &str) -> PathBuf {
        self.cache
            .join(MANIFEST_DIR)
            .join(format!("{}.json", family.to_ascii_lowercase()))
    }

    /// The library as `pull` and `run` read it for `input`, a `<family>[:<tag>]`: the whole document of a file source,
    /// nothing under `off`, and under a URL source the family's manifest (Q16), fetched every time (its body kept in
    /// the cache) with the cached copy standing in when the GET fails; a 404 is no library model, and an input that is
    /// no library name sends nothing. The library's family for `input`, when it has one.
    pub async fn lookup(&self, input: &str) -> Result<Library, RegistryError> {
        let LibrarySource::Url(_) = &self.source else {
            return self.cached(input);
        };
        let Some(family) = library_name(input) else {
            return Ok(Library::empty());
        };
        let url = self
            .source
            .manifest_url(family)
            .expect("a URL source has manifest URLs");
        let path = self.manifest_path(family);
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
        let family =
            LibraryManifest::parse(text, VERSION).map_err(|err| library_err(err.to_string()))?;
        // The bytes as fetched are kept once they read, so a body that does not never replaces a good copy.
        if fetched {
            write_atomically(&path, &body)?;
        }
        Ok(family.map_or_else(Library::empty, Library::single))
    }

    /// The library as the commands that send nothing read it for `input` (`list`, `show`, `rm`, `ps` and help): the
    /// whole document of a file source, nothing under `off`, and under a URL source the family's cached manifest, when
    /// a pull or run fetched one.
    pub fn cached(&self, input: &str) -> Result<Library, RegistryError> {
        match &self.source {
            LibrarySource::Off => Ok(Library::empty()),
            LibrarySource::File(path) => Library::from_file(path),
            LibrarySource::Url(_) => {
                let Some(family) = library_name(input) else {
                    return Ok(Library::empty());
                };
                let path = self.manifest_path(family);
                let text = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Library::empty());
                    }
                    Err(err) => return Err(RegistryError::Io { path, err }),
                };
                let family = LibraryManifest::parse(&text, VERSION).map_err(|err| {
                    RegistryError::Invalid {
                        what: path.display().to_string(),
                        msg: err.to_string(),
                    }
                })?;
                Ok(family.map_or_else(Library::empty, Library::single))
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

    /// The whole library read again, as `ardana serve` reads it on start and every refresh: a file source's
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

/// The family `input` names when it is a library name, `<family>[:<tag>]` split once at its first `:`, the family in
/// the name grammar; `None` for a reference (Q8): an input holding a `/`, one starting with `ollama:` and one ending in
/// `.gguf` ask the library nothing.
pub fn library_name(input: &str) -> Option<&str> {
    if input.contains('/')
        || input.starts_with(OLLAMA_PREFIX)
        || input.to_ascii_lowercase().ends_with(".gguf")
    {
        return None;
    }
    let family = input.split_once(':').map_or(input, |(family, _)| family);
    valid_name(family).then_some(family)
}

/// A library document as read: the families this `ardana` reads, the model requests use while nothing is pulled and
/// the first browser model.
#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// The model a request without a model uses when the registry is empty.
    pub default: String,
    /// The size with a browser variant that a playground offers first, a canonical name.
    pub browser_default: Option<String>,
    pub models: Vec<LibraryFamily>,
}

/// One quant of a library size: what a library name stands for (C4).
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryPick {
    /// The family's name.
    pub family: String,
    /// The size, with the family's `kind`, `summary`, `license` and `layout` where it carries none (Q4).
    pub size: LibrarySize,
    pub quant: GgufQuant,
}

impl LibraryPick {
    /// The canonical name (Q7): `<family>:<size>` for the size's default quant, `<family>:<size>-<quant>` for another.
    pub fn name(&self) -> String {
        self.size.name(&self.family, &self.quant)
    }

    /// The reference a pull reads (Q14): the quant's file in the size's GGUF repository, `hf.co/<org>/<repo>:<file>`.
    pub fn reference(&self) -> String {
        self.size.reference(&self.quant)
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

    /// The one family a manifest holds.
    fn single(family: LibraryFamily) -> Library {
        let mut library = Library::empty();
        library.models = vec![family];
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

    /// `document`'s families, as [`LibraryDocument::parse`] read them (Q13).
    pub fn from_document(document: LibraryDocument) -> Library {
        Library {
            default: document.default,
            browser_default: document.browser_default,
            models: document.models,
        }
    }

    /// Every size's default quant, in document order: one per size, under its canonical name `<family>:<size>`.
    pub fn sizes(&self) -> Vec<LibraryPick> {
        self.models
            .iter()
            .flat_map(|family| {
                family.sizes.iter().filter_map(|size| {
                    Some(LibraryPick {
                        family: family.name.clone(),
                        size: size.clone(),
                        quant: size.default_quant()?.clone(),
                    })
                })
            })
            .collect()
    }

    /// The canonical names of the sizes, in document order.
    pub fn names(&self) -> Vec<String> {
        self.sizes().iter().map(LibraryPick::name).collect()
    }

    /// The size named exactly `name`, its canonical name, when it has a browser variant (Q19).
    pub fn browser(&self, name: &str) -> Option<LibraryPick> {
        self.sizes()
            .into_iter()
            .find(|pick| pick.size.browser.is_some() && pick.name() == name)
    }

    /// The bytes a tab downloads to run the browser variant of the size `name`, when it has one.
    pub fn browser_size(&self, name: &str) -> Option<u64> {
        self.browser(name)?
            .size
            .browser
            .map(|browser| browser.bytes)
    }

    /// Whether `name` is the browser default, the browser model a playground offers first.
    pub fn is_browser_default(&self, name: &str) -> bool {
        self.browser_default.as_deref() == Some(name)
    }

    /// The quant the library name `input` names (Q8 to Q12): `Ok(None)` for a reference and for a family the library
    /// lacks; else the first of the family's tags (`latest`, `<size>` and `<size>-<quant>`, document order) equal to
    /// the input's tag, `latest` when it has none, family and tag compared ASCII case-insensitively and whole; a tag
    /// the family does not list is [`RegistryError::UnknownTag`], listing its canonical tags.
    pub fn find(&self, input: &str) -> Result<Option<LibraryPick>, RegistryError> {
        let Some(name) = library_name(input) else {
            return Ok(None);
        };
        let Some(family) = self
            .models
            .iter()
            .find(|family| family.name.eq_ignore_ascii_case(name))
        else {
            return Ok(None);
        };
        let wanted = input.split_once(':').map_or(LATEST, |(_, tag)| tag);
        let latest = wanted
            .eq_ignore_ascii_case(LATEST)
            .then(|| {
                let size = family
                    .sizes
                    .iter()
                    .find(|size| size.size == family.latest)?;
                Some((size, size.default_quant()?))
            })
            .flatten();
        let found = latest.or_else(|| {
            family.sizes.iter().find_map(|size| {
                let quant = size.gguf.quants.iter().find(|quant| {
                    size.tag(quant).eq_ignore_ascii_case(wanted)
                        || format!("{}-{}", size.size, quant.quant).eq_ignore_ascii_case(wanted)
                })?;
                Some((size, quant))
            })
        });
        let Some((size, quant)) = found else {
            return Err(RegistryError::UnknownTag {
                family: family.name.clone(),
                tag: wanted.to_string(),
                tags: family
                    .sizes
                    .iter()
                    .flat_map(|size| size.gguf.quants.iter().map(|quant| size.tag(quant)))
                    .collect(),
            });
        };
        Ok(Some(LibraryPick {
            family: family.name.clone(),
            size: size.clone(),
            quant: quant.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            parse(Some("https://ardana.ai/models.json")).manifest_url("Decider"),
            Some("https://ardana.ai/models/decider.json".into())
        );
        assert_eq!(LibrarySource::Off.manifest_url("decider"), None);
        assert_eq!(LibrarySource::Off.to_string(), "off");
    }

    /// Q8: a library name is `<family>[:<tag>]`, split once at its first `:`; references ask the library nothing.
    #[test]
    fn library_names_follow_the_grammar() {
        assert_eq!(library_name("decider"), Some("decider"));
        assert_eq!(library_name("Decider:2B-Q8_0"), Some("Decider"));
        assert_eq!(library_name("decider:a:b"), Some("decider"));
        assert_eq!(library_name("decider:"), Some("decider"));
        for other in [
            "",
            ":2b",
            " decider",
            "hf.co/Mapika/decider-2b-GGUF",
            "ollama:decider",
            "./decider",
            "decider.gguf",
            "Decider.GGUF",
        ] {
            assert_eq!(library_name(other), None, "{other:?}");
        }
    }

    #[test]
    fn causes_name_the_chain() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert_eq!(causes(&io), "refused");
        let err = LibraryError::Schema(2);
        assert!(causes(&err).contains("update ardana"));
    }
}
