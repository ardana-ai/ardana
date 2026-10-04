//! The HTTP server (compatible with Jev and TypeSafe clients), model lifecycle and the embedded playground.
//!
//! `ardana serve` answers `POST /v1/systemone`, `GET /v1/models` and `GET /health` for the models of an Ardana
//! registry and the model library ([`Models`]), and serves the browser variants of library models for the
//! playground's in-tab engine (`GET /v1/browser/<name>/<file>`). The binary passes in the registry and the
//! [`ardana_core::Runtimes`] it is built with, so this crate never sees a concrete runtime. Every other path serves
//! the playground embedded at build time. Every response keeps the page and what it loads to this origin (Q13).

mod body;
mod browser;
pub mod error;
pub mod models;

use std::sync::Arc;
use std::time::Duration;

use ardana_api::{ModelsResponse, SystemOneResponse};
use ardana_core::LoadOptions;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Args;
use memory_serve::CacheControl;
use serde_json::Value;
use tower_http::compression::CompressionLayer;

pub use error::ApiError;
pub use models::{ModelOptions, Models, ModelsError};

/// The largest request body: room for decider's 1,048,576-token request limit in escaped JSON, so an oversized
/// request is refused by its token count (413 naming the limit), not by its byte count.
pub const BODY_LIMIT: usize = 32 << 20;

/// `ardana serve` options.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ServeArgs {
    /// The address to listen on; 0.0.0.0 opens the API to your network
    #[arg(
        long,
        env = "ARDANA_HOST",
        hide_env_values = true,
        default_value = "127.0.0.1"
    )]
    pub host: String,
    /// The port to listen on
    #[arg(
        long,
        env = "ARDANA_PORT",
        hide_env_values = true,
        default_value_t = 8000
    )]
    pub port: u16,
    /// Require this key on /v1 (Authorization: Bearer <key>); without one the API is open
    #[arg(long, env = "ARDANA_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,
    /// The model for requests that name none (or Jev's jev-latest); default the first pulled model, else decider-2b
    #[arg(
        long,
        value_name = "MODEL",
        env = "ARDANA_DEFAULT_MODEL",
        hide_env_values = true
    )]
    pub default_model: Option<String>,
    /// How long an idle model stays loaded, e.g. 30s, 5m, 1h
    #[arg(long, env = "ARDANA_KEEP_ALIVE",
        hide_env_values = true, default_value = "5m", value_parser = parse_duration)]
    pub keep_alive: Duration,
    /// How many models stay loaded at once; one more unloads the least recently used
    #[arg(
        long,
        env = "ARDANA_MAX_LOADED_MODELS",
        hide_env_values = true,
        default_value_t = 1,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub max_loaded_models: u32,
    /// Scoring rows waiting at once (a question, or a score level read on its own, is a row); more gets 503
    #[arg(
        long,
        env = "ARDANA_MAX_QUEUED_ROWS",
        hide_env_values = true,
        default_value_t = 4096,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub max_queued_rows: u32,
}

impl ServeArgs {
    /// The lifecycle options these flags set, with the default [`LoadOptions`].
    pub fn model_options(&self) -> ModelOptions {
        ModelOptions {
            default_model: self.default_model.clone(),
            load: LoadOptions::default(),
            keep_alive: self.keep_alive,
            max_loaded_models: self.max_loaded_models as usize,
            max_queued_rows: self.max_queued_rows as usize,
        }
    }

    /// `host:port` to bind.
    pub fn addr(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// A duration as `<n>ms`, `<n>s`, `<n>m`, `<n>h` or plain seconds.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let n: u64 = number
        .parse()
        .map_err(|_| format!("{text:?} is not a duration like 300ms, 30s, 5m or 1h"))?;
    let secs = |factor: u64| Duration::from_secs(n.saturating_mul(factor));
    match unit {
        "ms" => Ok(Duration::from_millis(n)),
        "" | "s" => Ok(secs(1)),
        "m" => Ok(secs(60)),
        "h" => Ok(secs(3600)),
        _ => Err(format!("{text:?} has an unknown unit; use ms, s, m or h")),
    }
}

/// The API: `/health` (open) and the `/v1` routes, behind the key when one is set, plus the embedded playground. Unknown
/// `/v1/*` paths are 404 `{"detail":"Not Found"}` and never fall through to the playground. Every response carries
/// the cross-origin isolation headers ([`isolate`]).
pub fn router(models: Arc<Models>, api_key: Option<String>) -> Router {
    let mut v1 = Router::new()
        .route("/systemone", post(systemone))
        .route("/models", get(list_models))
        // A layer wraps only the routes added before it: the browser files go out as stored, with their length.
        .layer(CompressionLayer::new())
        .route("/browser/{name}/{file}", get(browser::file));
    if let Some(key) = api_key {
        v1 = v1.route_layer(middleware::from_fn_with_state(
            Arc::<str>::from(key),
            authenticate,
        ));
    }
    let v1 = v1
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(DefaultBodyLimit::max(BODY_LIMIT));
    Router::new()
        .route("/health", get(health))
        .nest("/v1", v1)
        .with_state(models)
        .merge(playground())
        .layer(middleware::map_response(isolate))
}

/// Q13: every response keeps its page in a browsing context group of its own (COOP), loads nothing another origin has
/// not opted into (COEP) and is not loaded by another origin (CORP). The page is then cross-origin isolated, so
/// onnxruntime-web's WASM backend can run on threads; no response carries a CORS header.
async fn isolate(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (name, value) in [
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-embedder-policy", "require-corp"),
        ("cross-origin-resource-policy", "same-origin"),
    ] {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

/// The playground `build.rs` embedded (or its placeholder), from memory: `index.html` at `/` and for every path no
/// file matches (the SPA fallback). HTML is never cached, so a new binary's page is picked up at once, and neither is
/// the in-tab engine ([`ENGINE`]); trunk's content-hashed assets, the fonts and onnxruntime-web's versioned directory
/// keep memory-serve's default cache time.
fn playground() -> Router {
    memory_serve::load!()
        .index_file(Some("/index.html"))
        .fallback(Some("/index.html"))
        .fallback_status(StatusCode::OK)
        .html_cache_control(CacheControl::NoCache)
        .into_router()
        .layer(middleware::from_fn(revalidate))
}

/// The in-tab engine's directory: the engine module trunk builds from `crates/ardana-engine` and the page's
/// `engine.js`, named the same in every build.
const ENGINE: &str = "/engine/";

/// Sends the files of [`ENGINE`] with `Cache-Control: no-cache`: a browser asks for them again on every load, and
/// memory-serve answers 304 while they are unchanged, so a page never runs with another binary's engine.
async fn revalidate(request: Request, next: Next) -> Response {
    let unhashed = request.uri().path().starts_with(ENGINE);
    let mut response = next.run(request).await;
    if unhashed {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    response
}

/// Serves `app` on `listener` until Ctrl-C or SIGTERM, lets the requests in flight finish, then unloads every model.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    models: Arc<Models>,
) -> std::io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    models.shutdown().await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        // Without a Ctrl-C handler the server still stops on SIGTERM.
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    // Windows has no SIGTERM: Ctrl-C stops the server there.
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

async fn systemone(
    State(models): State<Arc<Models>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<SystemOneResponse>, ApiError> {
    let body = body.map_err(ApiError::from_body)?;
    let request = body::parse(headers.get(header::CONTENT_TYPE), &body)?;
    Ok(Json(models.decide(request).await?))
}

async fn list_models(State(models): State<Arc<Models>>) -> Result<Json<ModelsResponse>, ApiError> {
    Ok(Json(models.list().await?))
}

async fn health(State(models): State<Arc<Models>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(models.health().await?))
}

/// Lets a request through when its bearer key equals `key`: no key is 403, another key (or scheme) 401.
async fn authenticate(State(key): State<Arc<str>>, request: Request, next: Next) -> Response {
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .map(|value| value.to_str().ok().and_then(bearer));
    match given {
        None | Some(Some("")) => ApiError::MissingKey.into_response(),
        Some(Some(given)) if same_key(given.as_bytes(), key.as_bytes()) => next.run(request).await,
        Some(_) => ApiError::WrongKey.into_response(),
    }
}

/// The key of an `Authorization: Bearer <key>` value; another scheme yields `None`.
fn bearer(value: &str) -> Option<&str> {
    let value = value.trim();
    let (scheme, key) = value.split_once(' ').unwrap_or((value, ""));
    scheme.eq_ignore_ascii_case("bearer").then(|| key.trim())
}

/// Compares keys in time independent of where they differ.
fn same_key(given: &[u8], key: &[u8]) -> bool {
    given.len() == key.len() && given.iter().zip(key).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}
