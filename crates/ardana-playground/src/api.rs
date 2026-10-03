//! The one place the playground talks to the network: the public API at `<base_url>/v1/*` (Q15). The base URL is the
//! page's own origin; no other host and no other path is ever called.

use std::fmt;

use ardana_api::{Detail, ErrorBody, ModelsResponse};
use js_sys::Uint8Array;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortSignal, Headers, ReadableStreamDefaultReader, ReadableStreamReadResult, Request,
    RequestInit, Response,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiClient {
    base_url: String,
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
    pub fn new(base_url: impl Into<String>) -> ApiClient {
        ApiClient {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// A client for the origin that served the page.
    pub fn same_origin() -> ApiClient {
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_default();
        ApiClient::new(origin)
    }

    /// `GET /v1/models`.
    pub async fn models(&self) -> Result<ModelsResponse, String> {
        let (status, text) = self.fetch("GET", "/v1/models", None).await?;
        if status != 200 {
            return Err(format!("GET /v1/models answered {status}: {text}"));
        }
        serde_json::from_str(&text)
            .map_err(|err| format!("GET /v1/models sent an unreadable list: {err}"))
    }

    /// `POST /v1/systemone` with `body`, timed with `performance.now()`.
    pub async fn systemone(&self, body: String) -> Exchange {
        let started = now();
        let response = self.fetch("POST", "/v1/systemone", Some(&body)).await;
        Exchange {
            latency_ms: now() - started,
            request: body,
            response: response.map_err(Unanswered::from),
            place: Place::Server,
        }
    }

    /// `GET /v1/browser/<name>/profile`: the profile of a model's browser variant, as JSON text, and the version of its
    /// files (the `ETag`). The server pulls the variant on the first request for it, so this waits for that pull;
    /// `signal` ends the wait.
    pub async fn browser_profile(
        &self,
        name: &str,
        signal: &AbortSignal,
    ) -> Result<(String, String), Failed> {
        let path = browser_path(name, "profile");
        let response = self
            .send("GET", &path, None, Some(signal), &[])
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
    /// whole file (200) when its copy is no longer that version.
    pub async fn browser_file(
        &self,
        name: &str,
        file: &str,
        from: u64,
        version: &str,
        signal: &AbortSignal,
    ) -> Result<Download, Failed> {
        let path = browser_path(name, file);
        let range = format!("bytes={from}-");
        let version = format!("\"{version}\"");
        let headers: &[(&str, &str)] = if from > 0 {
            &[("range", &range), ("if-range", &version)]
        } else {
            &[]
        };
        let response = self
            .send("GET", &path, None, Some(signal), headers)
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

    async fn fetch(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        let response = self.send(method, path, body, None, &[]).await?;
        Ok((response.status(), text(&response, method, path).await?))
    }

    /// Sends a request with `headers` and waits for the response's head; `signal`, when given, can end the request.
    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        signal: Option<&AbortSignal>,
        headers: &[(&str, &str)],
    ) -> Result<Response, String> {
        let init = RequestInit::new();
        init.set_method(method);
        init.set_signal(signal);
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
        let url = format!("{}{path}", self.base_url);
        let request = Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
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
