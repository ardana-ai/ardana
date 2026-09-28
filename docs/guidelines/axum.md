# axum guidelines

Scope: the HTTP layer of Ardana, which lives in `crates/ardana-server` (axum router, handlers, error mapping, model
lifecycle, auth, embedded playground) and is started by `ardana serve` in `crates/ardana`. It covers axum 0.8 with
tower-http 0.7 middleware, the tokio runtime underneath, the hand-off of blocking llama.cpp inference off the async
runtime, and serving the Leptos playground's `dist` from memory with `memory-serve`. `ardana-server` never depends on
`ardana-llama`; it receives a `Runtimes` value from the binary.

## Versions
- `axum` 0.8 — router, extractors and `axum::serve`; 0.8 changed path syntax to `/{param}` (docs.rs shows 0.8.9)
- `tower-http` 0.7 — compression middleware (`compression-br`, `compression-gzip`), all behind opt-in cargo features
  (docs.rs shows 0.7.1)
- `memory-serve` 2.4 — embeds `crates/ardana-playground/dist` into the binary; requires axum ^0.8
- `tokio` 1 (runtime) — multi-thread runtime, `TcpListener`, signals, `spawn_blocking`, `sync::mpsc`
- `tower` 0.5 (dev) — `ServiceExt::oneshot` in tests; needs the `util` feature
- `clap` 4 with `env` — `ardana_server::ServeArgs`, the `ardana serve` flags, reads `ARDANA_API_KEY`

## Rules
- Write routes in 0.8 syntax: `/{param}` and `/{*rest}`; the old `/:param` and `/*rest` forms panic at startup.
- Keep the route table explicit: `POST /v1/systemone`, `GET /v1/models`, `GET /health`, plus the playground. Put the
  `/v1` routes in their own `Router` mounted with `nest("/v1", ..)` and give that router its own `fallback` returning
  404 `{"detail":"Not Found"}`, so unknown `/v1/*` paths never fall through to the playground's `index.html`, plus a
  `method_not_allowed_fallback` returning 405 `{"detail":"Method Not Allowed"}` (FastAPI's bodies).
- Never call `.fallback(..)` on the top-level router: the memory-serve router carries the SPA fallback, and
  `Router::merge` panics when both merged routers have a fallback.
- Handlers are async functions that take extractors and return `Result<T, ApiError>` where both sides implement
  `IntoResponse`. Put the body extractor (`Json<T>`) last in the argument list; the body can be consumed once.
- Define one `ApiError` enum in `ardana-server` that implements `IntoResponse` and is the only place status codes and
  error bodies are built. Required mappings (W5 R5.4-R5.6):
  - validation (`DecideError::Invalid { loc, msg }` as type `value_error`, body errors) -> 422
    `{"detail":[{"loc","msg","type"}]}`
  - every other error is `{"detail":{"error_type","message"}}`, TypeSafe's live shape
  - context overflow and decider's row/token limits (`DecideError::Capacity`), and a body over the limit -> 413
    `request_too_large`, message contains "context window" for the context case; checked from the `Plan` before
    any load or decode
  - unknown `model` -> 404 `not_found_error` whose message lists `Registry::names()`
  - queue over `--max-queued-rows` -> 503 `overloaded_error` with `Retry-After: 1`
  - missing (or empty) bearer key -> 403, another key or scheme -> 401, both `authentication_error`
  - only the model files, the runtime and the worker (`DecideError::Runtime`, a panicked worker) may produce 5xx
    (`api_error`)
- Do not accept axum's default `Json` rejections: they answer 400 for a syntax error and 415 for a missing
  content type. `POST /v1/systemone` takes `Result<Bytes, BytesRejection>` and reads the body as decider's FastAPI
  does (`crates/ardana-server/src/body.rs`): JSON when the `Content-Type` is absent, `application/json` or
  `application/*+json`, any other type is `model_attributes_type`; an empty body is `missing` at `["body"]`; a syntax
  error is `json_invalid` at `["body", <char offset>]`; missing `state`/`questions` are `missing`, a non-object
  `questions` is `dict_type`, a non-string `model` is `string_type`. Use a rejection's `status()`/`body_text()` only
  to classify, never as the wire body.
- Set the body limit on purpose with `DefaultBodyLimit::max(BODY_LIMIT)` (32 MiB) on the `/v1` router; axum's default
  of 2 MB is below what decider's 1,048,576-token request limit needs. A body over the limit becomes a 413 with a
  JSON `detail`, not axum's plain-text body.
- Return headers with tuples, e.g. `(StatusCode::SERVICE_UNAVAILABLE, [(header::RETRY_AFTER, "1")], Json(body))`.
- Share state through `Router::with_state(AppState)` and the `State` extractor. `AppState` is a cheap `Clone`
  holding `Arc`s (registry, model manager, config); state is cloned per request, so never put large owned data in it.
- Wire auth as `middleware::from_fn_with_state` added with `route_layer` on the `/v1` router only, so unmatched paths
  still 404 instead of 401/403. `/health` and the playground stay open. Key comes from `--api-key` or
  `ARDANA_API_KEY` (clap `env`, `hide_env_values`); when unset the API is open and ignores `Authorization` headers.
  Compare keys without an early exit.
- The `ardana serve` flags are `ardana_server::ServeArgs` (`#[derive(clap::Args)]`), flattened into the binary's
  `serve` subcommand, so `crates/ardana-server/tests/http.rs` tests their defaults.
- Bind `127.0.0.1:8000` by default via `tokio::net::TcpListener::bind`; only `--host`/`--port` change it. Never
  default to `0.0.0.0`.
- Serve with `axum::serve(listener, app).with_graceful_shutdown(shutdown_signal())`, where `shutdown_signal` awaits
  `tokio::signal::ctrl_c()` or SIGTERM (`tokio::signal::unix`) under `tokio::select!`. On shutdown, stop admitting
  work, let queued decisions finish, then drop model workers.
- Middleware order: `Router::layer` wraps bottom-to-top, `tower::ServiceBuilder` runs top-to-bottom; prefer one
  `ServiceBuilder` per router so the order reads as written. Layers only apply to routes added before `.layer(..)`.
- tower-http layers in use: `CompressionLayer::new()` on the `/v1` router only (features `compression-br`,
  `compression-gzip`). Do not stack `CompressionLayer` over the memory-serve router; memory-serve already serves
  brotli/gzip variants itself. No `TraceLayer` yet: it only emits `tracing` events, and the binary installs no
  subscriber (`tracing-subscriber` is not a plan-named dependency); when one is added, never log request bodies,
  states can be private.
- No `CorsLayer`: the playground is same-origin (Q15). Add CORS only if a later decision allows other origins, and
  then with explicit origins, never `Any`.
- No `TimeoutLayer` on `/v1/systemone` shorter than a worst-case queued decision; admission control (503) bounds
  waiting, and a timeout would not stop inference already running.

## Inference off the async runtime
- Never call `LoadedModel::slot_logits`, `Runtime::load` or tokenizer-heavy planning inline in a handler; they block
  for milliseconds to seconds and starve the runtime.
- Give each loaded model one dedicated `std::thread` that owns the `Box<dyn LoadedModel>` (`Send`, needs `&mut`). Jobs
  go in over a bounded `tokio::sync::mpsc` channel (FIFO, capacity `--max-queued-rows`) received with
  `blocking_recv`; each job carries a `tokio::sync::oneshot` sender for its `Result<SystemOneResponse, DecideError>`.
  `blocking_recv` panics inside async code, so it runs only on that worker thread. The worker runs each job under
  `catch_unwind`, answers a panic as a runtime error and stops; a stopped worker is dropped from the loaded set.
- Plan every request (tokenizer, validation, 413 limits) before admission and before any load: each model's
  `Decider` (tokenizer and profile) is built on its first request and kept apart from its weights.
- Admission counts queued rows, not requests: keep an atomic queued-row counter per server, reject with 503 before
  loading or enqueueing when `queued + plan.rows > max_queued_rows`, and decrement when the request ends (a guard's
  `Drop`). Treat `TrySendError::Full` as the same 503.
- Use `tokio::task::spawn_blocking` only for bounded one-off work (model load, `hf`/registry file reads). Its tasks
  cannot be aborted once running, and runtime shutdown waits for them.
- LRU eviction and the `--keep-alive` idle unload close the worker's channel; the worker drops the model after its
  queue drains, so eviction never cancels an admitted request. Loads run one at a time under the loaded-set lock.
  A request holds a lease (in-flight count, last use) from acquiring the worker to its answer; one tokio task per
  loaded model sleeps until `last use + keep_alive` and unloads the model only when it holds no lease, keeping only
  `Weak` references so it never keeps a model alive.

## Embedded playground (memory-serve)
- List `memory-serve` in both `[dependencies]` and `[build-dependencies]` of `ardana-server`; the build script and
  the runtime both use it.
- `crates/ardana-server/build.rs` calls `memory_serve::load_directory(path)` with the playground `dist` directory,
  or the directory `ARDANA_PLAYGROUND_DIST` names (R6.4). If `index.html` is missing there, it prints
  `cargo::warning=no playground at <dir> ...` and loads the checked-in `crates/ardana-server/placeholder/` instead;
  never fail the build. Emit `cargo::rerun-if-env-changed` for the override and `cargo::rerun-if-changed` for the
  dist path; memory-serve's docs do not promise rebuild tracking.
- memory-serve prints every embedded asset as a cargo warning unless `MEMORY_SERVE_QUIET=1`; `.cargo/config.toml`
  `[env]` sets it, so the only build warning left is the missing-dist one.
- `build.rs` never runs trunk (rust-lang/cargo#8938 deadlock); `cargo xtask build` builds `dist` first, then the
  release `ardana`.
- `ardana_server::router` merges the playground router, built with `memory_serve::load!().index_file(Some("/index.html"))`
  `.fallback(Some("/index.html")).fallback_status(StatusCode::OK).html_cache_control(CacheControl::NoCache)`
  `.into_router()`; the binary calls `router` once. `into_router` leaks memory by design (tests may leak a little).
  `fallback_status` defaults to 404, so set it for the SPA. The `/v1` router's own fallback keeps unknown `/v1/*`
  paths on the API's 404.
- `ardana-server` depends on memory-serve with `force-embed`, so debug builds embed too and every binary serves from
  memory, never from the source tree (R6.3). `crates/ardana-server/tests/playground.rs` checks that `/`, unknown
  paths and every asset come from the directory `build.rs` embedded.

## Testing
- Test handlers in `crates/ardana-server/tests/*.rs` without binding a port: build the app with a fake `Runtime`
  and call `app.clone().oneshot(Request::builder()...body(Body::empty())?)` from `tower::ServiceExt`; read bodies
  with `axum::body::to_bytes(body, usize::MAX)`. The fake runtime (`tests/common/mod.rs`) counts loads and drops and
  can hold decodes; requests are read with decider-2b's real tokenizer and profile from `tmp/hf`.
- Assert status, headers (`Retry-After`, `Content-Type`) and the exact JSON shape of every error body, including
  that no bad-input case returns 5xx.
- Fake runtimes are for lifecycle and error-path tests only; end-to-end checks (`cargo xtask e2e jevcompat|sdk|
  jevbench`) run the release binary on decider-2b pulled offline from `tmp/hf`, each in its own `ARDANA_HOME` under
  `tmp/e2e/<suite>`, on a free port of 127.0.0.1, stopped with SIGTERM afterwards.

## Sources
- https://docs.rs/axum/0.8 — crate overview, handlers, extractors, state sharing, `axum::serve`, tokio features
- https://docs.rs/axum/0.8/axum/struct.Router.html — path syntax, nest, merge, fallback, layer vs route_layer
- https://docs.rs/axum/0.8/axum/routing/struct.Router.html#method.nest — nested router fallback inheritance, merge panic
- https://docs.rs/axum/0.8/axum/extract/index.html — extractor order, rejection handling, 2 MB body default
- https://docs.rs/axum/0.8/axum/extract/struct.DefaultBodyLimit.html — `max`, `disable`, 413 on overflow
- https://docs.rs/axum/0.8/axum/extract/rejection/enum.JsonRejection.html — rejection variants, `status`, `body_text`
- https://github.com/tokio-rs/axum/blob/main/axum/src/extract/rejection.rs — 422/400/415 statuses of JSON rejections
- https://docs.rs/axum/0.8/axum/error_handling/index.html — infallible services, `Result<T, E: IntoResponse>`
- https://docs.rs/axum/0.8/axum/response/index.html — `IntoResponse`, status/header tuples
- https://docs.rs/axum/0.8/axum/middleware/index.html — `from_fn_with_state`, layer ordering
- https://docs.rs/axum/0.8/axum/serve/struct.Serve.html — `with_graceful_shutdown`
- https://github.com/tokio-rs/axum/blob/main/examples/graceful-shutdown/src/main.rs — ctrl_c/SIGTERM shutdown signal
- https://github.com/tokio-rs/axum/blob/main/examples/testing/src/main.rs — `oneshot` tests without a port
- https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0 — 0.8 breaking changes (path syntax, `Option<T>`, no async_trait)
- https://docs.rs/tower-http/0.7 — feature flags, all middleware opt-in
- https://docs.rs/tower-http/0.7/tower_http/trace/index.html — `TraceLayer::new_for_http`
- https://docs.rs/tower-http/0.7/tower_http/compression/index.html — `CompressionLayer`, compression features
- https://docs.rs/tower-http/0.7/tower_http/compression/predicate/struct.DefaultPredicate.html — what is not compressed
- https://docs.rs/tower-http/0.7/tower_http/cors/index.html — `CorsLayer`, risks of `Any`
- https://docs.rs/tower/latest/tower/trait.ServiceExt.html — `oneshot`, `ready`, `util` feature
- https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html — blocking pool, no abort, dedicated threads for long work
- https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html — bounded FIFO channel, `try_send`, `blocking_recv`
- https://docs.rs/memory-serve/2.4 — build.rs loading, debug disk reads, `force-embed`, brotli in release
- https://docs.rs/memory-serve/2.4/memory_serve/struct.MemoryServe.html — `fallback`, `fallback_status`, `into_router`
- https://docs.rs/memory-serve/2.4/memory_serve/fn.load_directory.html — `load_directory` signature
- https://github.com/tweedegolf/memory-serve — README: both dependency sections, `load!()` and router merge
