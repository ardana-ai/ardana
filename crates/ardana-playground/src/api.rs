//! The one place the playground talks to the network. Served by `ardana serve`, it calls the public API on the page's
//! own origin, `/v1/*` (Q15), and no other host or path. The standalone build (`cargo xtask build-playground`), which no
//! server serves, reads the model library document (C1) instead, from the URL its build named (`--library`, default
//! `/models.json`), on every list, and fetches the browser files from their Hugging Face repositories at the commits
//! the document pins (Q13).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use ardana_api::{
    BrowserEntry, Detail, ErrorBody, LibraryDocument, LibraryEntry, ModelInfo, ModelsResponse,
};
use js_sys::Uint8Array;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortSignal, Headers, ReadableStreamDefaultReader, ReadableStreamReadResult, ReferrerPolicy,
    Request, RequestCache, RequestInit, Response,
};

/// The version of this page, which reads a library document as the `ardana` of the same version does (Q12).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The prefix of a Hugging Face repository in the library document, `hf.co/<org>/<repo>`.
const HF_PREFIX: &str = "hf.co/";

#[derive(Debug, Clone)]
pub struct ApiClient {
    /// The standalone build's library, in place of a server; none in the build `ardana serve` embeds.
    library: Option<Library>,
}

/// Where the standalone build reads the library: its document and the Hub its browser files come from, both named by
/// the build, and the browser variants the last document listed, which a run in the tab reads its files by.
#[derive(Debug, Clone)]
struct Library {
    /// The document's URL (`cargo xtask build-playground --library`), on the page's origin or whole.
    url: &'static str,
    /// The Hugging Face the browser files come from (`--hub`): `https://huggingface.co`, or a stand-in.
    hub: &'static str,
    /// Each browser variant of the last document, by its model's name; shared by every clone of the client.
    browser: Rc<RefCell<BTreeMap<String, BrowserEntry>>>,
}

/// One run as it happened: a `POST /v1/systemone` round trip, or the same request answered in this tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    /// The exact body sent.
    pub request: String,
    /// The response status and exact body text, or why no response arrived.
    pub response: Result<(u16, String), Unanswered>,
    /// Milliseconds from just before the request to the last byte of the response body; in the tab, the plan, the
    /// decodes and the readout, without the files' download.
    pub latency_ms: f64,
    /// Where the request was answered.
    pub place: Place,
}

/// Why a run got no response: the reason as the browser, the server or the engine gave it, and for a run in this tab,
/// what failed and what to do next, in the page's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unanswered {
    pub reason: String,
    pub advice: Option<String>,
}

impl From<String> for Unanswered {
    fn from(reason: String) -> Unanswered {
        Unanswered {
            reason,
            advice: None,
        }
    }
}

/// Why a browser file (or its profile) did not arrive whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// The server could not be reached, or its answer stopped arriving.
    Connection(String),
    /// The server answered with an error: its status and message.
    Refused(String),
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failed::Connection(reason) | Failed::Refused(reason) => f.write_str(reason),
        }
    }
}

/// Where a run is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// `POST /v1/systemone` on the server.
    Server,
    /// The in-tab engine, on its backend once a session exists (a refused request needs none).
    Tab(Option<Backend>),
}

/// The onnxruntime-web execution provider a tab's session runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    WebGpu,
    Wasm,
}

impl Backend {
    /// The name onnxruntime-web knows it by.
    pub fn provider(self) -> &'static str {
        match self {
            Backend::WebGpu => "webgpu",
            Backend::Wasm => "wasm",
        }
    }

    /// The name the page shows.
    pub fn name(self) -> &'static str {
        match self {
            Backend::WebGpu => "WebGPU",
            Backend::Wasm => "WASM",
        }
    }
}

impl ApiClient {
    /// The page's client: the server that served it, or the standalone build's library.
    pub fn page() -> ApiClient {
        ApiClient { library: library() }
    }

    /// Whether this is the standalone build, which no server serves: a model runs in this tab or with the ardana CLI on
    /// the visitor's machine.
    pub fn standalone(&self) -> bool {
        self.library.is_some()
    }

    /// `GET /v1/models`; in the standalone build, the library document fetched now and read as this version reads it
    /// (Q12: an entry it cannot read is left out), listed as a server that has pulled none lists the library, with no
    /// default model and every browser variant held. A document that does not arrive or read leaves the last list in
    /// place and says the library is unavailable.
    pub async fn models(&self) -> Result<ModelsResponse, String> {
        let Some(library) = &self.library else {
            let (status, text) = self.fetch("GET", "/v1/models", None, false).await?;
            if status != 200 {
                return Err(format!("GET /v1/models answered {status}: {text}"));
            }
            return serde_json::from_str(&text)
                .map_err(|err| format!("GET /v1/models sent an unreadable list: {err}"));
        };
        let unavailable = |reason: String| format!("The model library is unavailable: {reason}");
        let (status, text) = self
            .fetch("GET", library.url, None, true)
            .await
            .map_err(unavailable)?;
        if status != 200 {
            return Err(unavailable(format!(
                "GET {} answered {status}",
                library.url
            )));
        }
        let document =
            LibraryDocument::parse(&text, VERSION).map_err(|err| unavailable(err.to_string()))?;
        *library.browser.borrow_mut() = document
            .models
            .iter()
            .filter_map(|entry| Some((entry.name.clone(), entry.browser.clone()?)))
            .collect();
        Ok(listed(&document))
    }

    /// `POST /v1/systemone` with `body`, timed with `performance.now()`.
    pub async fn systemone(&self, body: String) -> Exchange {
        let started = now();
        let response = self
            .fetch("POST", "/v1/systemone", Some(&body), false)
            .await;
        Exchange {
            latency_ms: now() - started,
            request: body,
            response: response.map_err(Unanswered::from),
            place: Place::Server,
        }
    }

    /// `GET /v1/browser/<name>/profile`: the profile of a model's browser variant, as JSON text, and the version of its
    /// files (the `ETag`). The server pulls the variant on the first request for it, so this waits for that pull;
    /// `signal` ends the wait. In the standalone build, the document's profile and the commit of the variant's files.
    pub async fn browser_profile(
        &self,
        name: &str,
        signal: &AbortSignal,
    ) -> Result<(String, String), Failed> {
        if let Some(library) = &self.library {
            return library
                .variant(name)
                .map(|variant| (variant.profile.to_string(), variant.commit));
        }
        let path = browser_path(name, "profile");
        let response = self
            .send("GET", &path, None, Some(signal), &[], false)
            .await
            .map_err(Failed::Connection)?;
        let version = response
            .headers()
            .get("etag")
            .ok()
            .flatten()
            .map(|etag| etag.trim_matches('"').to_string())
            .unwrap_or_default();
        let status = response.status();
        let text = text(&response, "GET", &path)
            .await
            .map_err(Failed::Connection)?;
        if status != 200 {
            return Err(Failed::Refused(refusal(status, &text)));
        }
        Ok((text, version))
    }

    /// `GET /v1/browser/<name>/<file>`, its bytes to be read as they arrive ([`Download::next`]); `signal` stops the
    /// download where it is. From byte `from` on when the tab keeps the bytes before it, of the file at `version`:
    /// `Range: bytes=<from>-` with `If-Range: "<version>"`, which the server answers with the rest (206), or with the
    /// whole file (200) when its copy is no longer that version. In the standalone build, the file of the variant's
    /// repository at that commit, `<hub>/<org>/<repo>/resolve/<version>/<file>`, which never changes: `Range` alone.
    pub async fn browser_file(
        &self,
        name: &str,
        file: &str,
        from: u64,
        version: &str,
        signal: &AbortSignal,
    ) -> Result<Download, Failed> {
        let path = match &self.library {
            Some(library) => {
                let variant = library.variant(name)?;
                let repository = variant
                    .weights
                    .strip_prefix(HF_PREFIX)
                    .unwrap_or(&variant.weights);
                let hub = library.hub.trim_end_matches('/');
                format!("{hub}/{repository}/resolve/{version}/{file}")
            }
            None => browser_path(name, file),
        };
        let range = format!("bytes={from}-");
        let version = format!("\"{version}\"");
        // The Hub's `ETag` is no commit, and a request with `If-Range` would be no CORS-safelisted one there.
        let headers: &[(&str, &str)] = match (from > 0, &self.library) {
            (false, _) => &[],
            (true, Some(_)) => &[("range", &range)],
            (true, None) => &[("range", &range), ("if-range", &version)],
        };
        let response = self
            .send("GET", &path, None, Some(signal), headers, false)
            .await
            .map_err(Failed::Connection)?;
        let status = response.status();
        let header = |name: &str| response.headers().get(name).ok().flatten();
        let (start, length) = match status {
            200 => {
                let length = header("content-length")
                    .and_then(|length| length.parse::<u64>().ok())
                    .ok_or_else(|| Failed::Refused(format!("GET {path} sent no length")))?;
                (0, length)
            }
            206 if from > 0 => {
                let sent = header("content-range").unwrap_or_default();
                // The rest of the file, from the byte asked for: `bytes <from>-<length - 1>/<length>`.
                let rest = sent
                    .strip_prefix("bytes ")
                    .and_then(|range| range.split_once('/'))
                    .and_then(|(range, length)| {
                        Some((range.split_once('-')?, length.parse::<u64>().ok()?))
                    })
                    .filter(|((start, end), length)| {
                        start.parse::<u64>() == Ok(from)
                            && end.parse::<u64>().ok() == length.checked_sub(1)
                    });
                let Some((_, length)) = rest else {
                    return Err(Failed::Refused(format!(
                        "GET {path} from byte {from} sent the bytes {sent:?}"
                    )));
                };
                (from, length)
            }
            _ => {
                let text = text(&response, "GET", &path)
                    .await
                    .map_err(Failed::Connection)?;
                return Err(Failed::Refused(refusal(status, &text)));
            }
        };
        let body = response
            .body()
            .ok_or_else(|| Failed::Refused(format!("GET {path} sent no body")))?;
        let reader = ReadableStreamDefaultReader::new(&body).map_err(|err| {
            Failed::Connection(format!("downloading {file} stopped: {}", js_error(err)))
        })?;
        Ok(Download {
            start,
            length,
            file: file.to_string(),
            path,
            reader,
            read: 0,
        })
    }

    /// The status and whole body of a request; `fresh` asks the browser's cache for nothing it has not revalidated.
    async fn fetch(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        fresh: bool,
    ) -> Result<(u16, String), String> {
        let response = self.send(method, path, body, None, &[], fresh).await?;
        Ok((response.status(), text(&response, method, path).await?))
    }

    /// Sends a request to `path` (on the page's origin, or a whole URL) with `headers` and waits for the response's head;
    /// `signal`, when given, can end the request. `fresh` sends it `cache: no-cache`: a copy the browser's cache holds
    /// is used only once the host says it still stands (the library document, which changes under one URL; its host
    /// answers an `If-None-Match` with a 304), never on the cache's own guess.
    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        signal: Option<&AbortSignal>,
        headers: &[(&str, &str)],
        fresh: bool,
    ) -> Result<Response, String> {
        let init = RequestInit::new();
        init.set_method(method);
        init.set_signal(signal);
        if fresh {
            init.set_cache(RequestCache::NoCache);
        }
        // The standalone build asks the Hub alone, whose CDN refuses some referring pages (any on *.workers.dev: a 404
        // without CORS headers); it names none.
        if self.library.is_some() {
            init.set_referrer_policy(ReferrerPolicy::NoReferrer);
        }
        let sent = Headers::new().map_err(js_error)?;
        for (name, value) in headers {
            sent.set(name, value).map_err(js_error)?;
        }
        if let Some(body) = body {
            sent.set("content-type", "application/json")
                .map_err(js_error)?;
            init.set_body(&JsValue::from_str(body));
        }
        init.set_headers(&sent);
        let request = Request::new_with_str_and_init(path, &init).map_err(js_error)?;
        let window = web_sys::window().ok_or("no window")?;
        JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|err| {
                format!(
                    "{method} {path} did not reach the server: {}",
                    js_error(err)
                )
            })?
            .dyn_into()
            .map_err(js_error)
    }
}

/// A browser file as the server sends it, read as it arrives: the whole file, or the rest of it from the byte a
/// download that stopped had reached.
pub struct Download {
    /// Where the bytes the response carries start: 0 for the whole file, else the bytes the tab has.
    pub start: u64,
    /// The bytes of the whole file.
    pub length: u64,
    file: String,
    path: String,
    reader: ReadableStreamDefaultReader,
    /// The bytes read so far, from `start` on.
    read: u64,
}

impl Download {
    /// The bytes that arrived next; `None` once the file is whole.
    pub async fn next(&mut self) -> Result<Option<Uint8Array>, Failed> {
        let chunk: ReadableStreamReadResult = JsFuture::from(self.reader.read())
            .await
            .map_err(|err| {
                Failed::Connection(format!(
                    "downloading {} stopped: {}",
                    self.file,
                    js_error(err)
                ))
            })?
            .unchecked_into();
        let had = self.start + self.read;
        if chunk.get_done() == Some(true) {
            if had != self.length {
                return Err(Failed::Connection(format!(
                    "downloading {} stopped after {had} of {} bytes",
                    self.file, self.length
                )));
            }
            return Ok(None);
        }
        let chunk: Uint8Array = chunk.get_value().unchecked_into();
        if had + u64::from(chunk.length()) > self.length {
            return Err(Failed::Refused(format!(
                "GET {} sent more than its {} bytes",
                self.path, self.length
            )));
        }
        self.read += u64::from(chunk.length());
        Ok(Some(chunk))
    }
}

impl Library {
    /// The browser variant of `name` as the last document listed it.
    fn variant(&self, name: &str) -> Result<BrowserEntry, Failed> {
        self.browser.borrow().get(name).cloned().ok_or_else(|| {
            Failed::Refused(format!(
                "the model library names no browser files of {name}"
            ))
        })
    }
}

/// Where the standalone build reads its library: the URLs `cargo xtask build-playground` named (`--library`, `--hub`),
/// set for the build; none in the build `ardana serve` embeds.
#[cfg(feature = "standalone")]
fn library() -> Option<Library> {
    Some(Library {
        url: env!(
            "ARDANA_PLAYGROUND_MODELS_URL",
            "the standalone playground is built by `cargo xtask build-playground`"
        ),
        hub: env!(
            "ARDANA_PLAYGROUND_HUB_URL",
            "the standalone playground is built by `cargo xtask build-playground`"
        ),
        browser: Rc::default(),
    })
}

#[cfg(not(feature = "standalone"))]
fn library() -> Option<Library> {
    None
}

/// `document`'s models as `GET /v1/models` lists the library on a server that has pulled none (`x_pulled: false`,
/// the GGUF's size), with no default model (no server runs one here) and every browser variant held (the Hub holds it
/// whole), in the document's order.
fn listed(document: &LibraryDocument) -> ModelsResponse {
    let info = |entry: &LibraryEntry| ModelInfo {
        name: entry.name.clone(),
        description: format!("{}:{}", entry.weights, entry.quant),
        release_date: entry.release_date.clone(),
        x_pulled: Some(false),
        x_default: false,
        x_size: Some(entry.size),
        x_browser: entry.browser.as_ref().map(|browser| browser.size),
        x_browser_pulled: entry.browser.is_some(),
        x_browser_default: document.browser_default.as_deref() == Some(&entry.name),
    };
    ModelsResponse {
        models: document.models.iter().map(info).collect(),
    }
}

/// `/v1/browser/<name>/<file>`, the name percent-encoded.
fn browser_path(name: &str, file: &str) -> String {
    let name = String::from(js_sys::encode_uri_component(name));
    format!("/v1/browser/{name}/{file}")
}

/// The whole body of `response` as text.
async fn text(response: &Response, method: &str, path: &str) -> Result<String, String> {
    let text = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(|err| {
            format!(
                "{method} {path}: the response body was cut off: {}",
                js_error(err)
            )
        })?;
    Ok(text.as_string().unwrap_or_default())
}

/// Why the server refused a browser file: its error message, else the body as it came.
pub fn refusal(status: u16, body: &str) -> String {
    match serde_json::from_str::<ErrorBody>(body).map(|body| body.detail) {
        Ok(Detail::Error { message, .. }) => format!("HTTP {status}: {message}"),
        Ok(Detail::Text(text)) => format!("HTTP {status}: {text}"),
        _ => format!("HTTP {status}: {body}"),
    }
}

pub fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

pub fn js_error(value: JsValue) -> String {
    value
        .dyn_ref::<js_sys::Error>()
        .map(|err| String::from(err.message()))
        .or_else(|| value.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R6.2: the standalone build lists the document's entries in its order, each as a server that has pulled none
    /// would (its GGUF's size, its browser variant's bytes and held), the browser default marked and no default model.
    #[test]
    fn the_document_lists_as_a_server_would() {
        let entry = |name: &str, browser: &str| {
            format!(
                r#"{{"name": "{name}", "kind": "decider", "summary": "S.", "base": "o/b", "params": "1B",
                    "weights": "hf.co/o/{name}-GGUF", "commit": "{}", "quant": "Q4_K_M", "size": 10,
                    "release_date": "2026-01-01", "tags": []{browser}}}"#,
                "a".repeat(40)
            )
        };
        let text = format!(
            r#"{{"schema": 1, "default": "b", "browser_default": "a", "models": [{}, {}]}}"#,
            entry(
                "a",
                r#", "browser": {"weights": "hf.co/o/a-ONNX", "commit": "c", "quant": "int4", "size": 2,
                    "profile": {}}"#
            ),
            entry("b", "")
        );
        let document = LibraryDocument::parse(&text, VERSION).unwrap();
        assert_eq!(
            listed(&document).models,
            [
                ModelInfo {
                    name: "a".into(),
                    description: "hf.co/o/a-GGUF:Q4_K_M".into(),
                    release_date: "2026-01-01".into(),
                    x_pulled: Some(false),
                    x_default: false,
                    x_size: Some(10),
                    x_browser: Some(2),
                    x_browser_pulled: true,
                    x_browser_default: true,
                },
                ModelInfo {
                    name: "b".into(),
                    description: "hf.co/o/b-GGUF:Q4_K_M".into(),
                    release_date: "2026-01-01".into(),
                    x_pulled: Some(false),
                    x_default: false,
                    x_size: Some(10),
                    x_browser: None,
                    x_browser_pulled: false,
                    x_browser_default: false,
                },
            ]
        );
    }

    #[test]
    fn refusals_name_the_server_message() {
        assert_eq!(
            refusal(
                500,
                r#"{"detail":{"error_type":"api_error","message":"pulling the browser variant of x: offline"}}"#
            ),
            "HTTP 500: pulling the browser variant of x: offline"
        );
        assert_eq!(
            refusal(404, r#"{"detail":"Not Found"}"#),
            "HTTP 404: Not Found"
        );
        assert_eq!(refusal(502, "Bad gateway"), "HTTP 502: Bad gateway");
    }
}
