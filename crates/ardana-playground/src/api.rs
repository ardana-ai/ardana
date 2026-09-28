//! The one place the playground talks to the network: the public API at `<base_url>/v1/*` (Q15). The base URL is the
//! page's own origin; no other host and no other path is ever called.

use ardana_api::ModelsResponse;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, Response};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiClient {
    base_url: String,
}

/// One `POST /v1/systemone` round trip as it happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    /// The exact body sent.
    pub request: String,
    /// The response status and exact body text, or why no response arrived.
    pub response: Result<(u16, String), String>,
    /// Milliseconds from just before the request to the last byte of the response body.
    pub latency_ms: f64,
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
            response,
        }
    }

    async fn fetch(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        let init = RequestInit::new();
        init.set_method(method);
        if let Some(body) = body {
            let headers = Headers::new().map_err(js_error)?;
            headers
                .set("content-type", "application/json")
                .map_err(js_error)?;
            init.set_headers(&headers);
            init.set_body(&JsValue::from_str(body));
        }
        let url = format!("{}{path}", self.base_url);
        let request = Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
        let window = web_sys::window().ok_or("no window")?;
        let response: Response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|err| {
                format!(
                    "{method} {path} did not reach the server: {}",
                    js_error(err)
                )
            })?
            .dyn_into()
            .map_err(js_error)?;
        let text = JsFuture::from(response.text().map_err(js_error)?)
            .await
            .map_err(|err| {
                format!(
                    "{method} {path}: the response body was cut off: {}",
                    js_error(err)
                )
            })?;
        Ok((response.status(), text.as_string().unwrap_or_default()))
    }
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

fn js_error(value: JsValue) -> String {
    value
        .dyn_ref::<js_sys::Error>()
        .map(|err| String::from(err.message()))
        .or_else(|| value.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}
