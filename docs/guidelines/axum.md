# axum guidelines

Scope: the HTTP layer of Ardana, which lives in `crates/ardana-server` (axum router, handlers, error mapping, model
lifecycle, auth, embedded playground, the browser variants' files, the public mode) and is started by `ardana serve`
(or `ardana serve --public`) in `crates/ardana`. It covers axum 0.8 with tower-http 0.7 middleware, the tokio runtime
underneath, the hand-off of blocking llama.cpp inference off the async runtime, serving the Leptos playground's `dist`
from memory with `memory-serve`, and serving library models' browser variants from the hub cache (`/v1/browser`).
`ardana-server` never depends on `ardana-llama`; it receives a `Runtimes` value from the binary.

## Versions
- `axum` 0.8 — router, extractors and `axum::serve`; 0.8 changed path syntax to `/{param}` (docs.rs shows 0.8.9)
- `tower-http` 0.7 — compression middleware (`compression-br`, `compression-gzip`), all behind opt-in cargo features
  (docs.rs shows 0.7.1)
- `memory-serve` 2.4 — embeds `crates/ardana-playground/dist` into the binary; requires axum ^0.8
- `tokio` 1 (runtime) — multi-thread runtime, `TcpListener`, signals, `spawn_blocking`, `sync::mpsc`, and `fs` (a
  feature that adds no crate) and `io-util` (`AsyncSeekExt`; it enables `bytes`, already in the build) for the
  browser files and their byte ranges
- `http-body` 1 — the `Body` trait (`Frame`, `SizeHint`) the browser files' streaming body implements; already in the
  build through axum, named directly because axum re-exports the trait but not `Frame`
- `tower` 0.5 (dev) — `ServiceExt::oneshot` in tests; needs the `util` feature
- `clap` 4 with `env` — `ardana_server::ServeArgs`, the `ardana serve` flags, reads `ARDANA_API_KEY`, and
  `ARDANA_PUBLIC` through `clap::builder::BoolishValueParser` (`1`, `true`, `yes`, `on`; a bare flag's default parser
  takes only `true`/`false`)

## Rules
- Write routes in 0.8 syntax: `/{param}` and `/{*rest}`; the old `/:param` and `/*rest` forms panic at startup.
- Keep the route table explicit: `POST /v1/systemone`, `GET /v1/models`, `GET /v1/browser/{name}/{file}`,
  `GET /health`, plus the playground; a public server routes `POST /v1/systemone` to a handler that takes no body.
  Put the `/v1` routes in their own `Router` mounted with `nest("/v1", ..)` and give that router its own `fallback`
  returning 404 `{"detail":"Not Found"}`, so unknown `/v1/*` paths never fall through to the playground's
  `index.html`, plus a `method_not_allowed_fallback` returning 405 `{"detail":"Method Not Allowed"}` (FastAPI's
  bodies).
- Q13: every response, the playground's and the API's alike, carries `Cross-Origin-Opener-Policy: same-origin`,
  `Cross-Origin-Embedder-Policy: require-corp` and `Cross-Origin-Resource-Policy: same-origin`, set by one
  `middleware::map_response` layer on the merged top-level router (`lib.rs#isolate`), and no CORS header. The page is
  then cross-origin isolated (onnxruntime-web's WASM backend runs on threads), loads nothing another origin has not
  opted into, and nothing of the server loads in another origin's page.
- Never call `.fallback(..)` on the top-level router: the memory-serve router carries the SPA fallback, and
  `Router::merge` panics when both merged routers have a fallback.
- Handlers are async functions that take extractors and return `Result<T, ApiError>` where both sides implement
  `IntoResponse`. Put the body extractor (`Json<T>`) last in the argument list; the body can be consumed once.
- Define one `ApiError` enum in `ardana-server` that implements `IntoResponse` and is the only place the server builds
  status codes and error bodies, from `ardana-api`'s wire shapes (`ErrorBody`, `ValidationItem`). A `DecideError` is
  answered as `ardana-core` words it (`DecideError::status`, `DecideError::body`, through `From<DecideError> for
  ApiError`): the playground's in-tab engine (its engine module, `crates/ardana-engine`) answers a refused request
  with the same function, so the two bodies are byte for byte the same. Required mappings (W5 R5.4-R5.6):
  - validation (`DecideError::Invalid { loc, msg }` as type `value_error`, body errors) -> 422
    `{"detail":[{"loc","msg","type"}]}`
  - every other error is `{"detail":{"error_type","message"}}`, TypeSafe's live shape
  - context overflow and decider's row/token limits (`DecideError::Capacity`), and a body over the limit -> 413
    `request_too_large`, message contains "context window" for the context case; checked from the `Plan` before
    any load or decode
  - unknown `model` -> 404 `not_found_error` whose message lists `Registry::names()`
  - queue over `--max-queued-rows` -> 503 `overloaded_error` with `Retry-After: 1`
  - missing (or empty) bearer key -> 403, another key or scheme -> 401, both `authentication_error`
  - any decision on a public server (`ApiError::RunsNoModel`) -> 403 `permission_error`, whatever its body: it is
    not an authentication problem (no key opens it), and TypeSafe's SDKs read a 403 as permission denied
    (`TypeSafePermissionDeniedError`)
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
  `serve` subcommand, so `crates/ardana-server/tests/http.rs` tests their defaults; `--public` sets
  `ModelOptions::public`, which in-process tests set directly (they cannot set variables).
- Bind `127.0.0.1:8000` by default via `tokio::net::TcpListener::bind`; only `--host`/`--port` change it. Never
  default to `0.0.0.0`.
- Serve with `axum::serve(listener, app).with_graceful_shutdown(shutdown_signal())`, where `shutdown_signal` awaits
  `tokio::signal::ctrl_c()` or SIGTERM (`tokio::signal::unix`) under `tokio::select!`. On shutdown, stop admitting
  work, let queued decisions finish, then drop model workers.
- Middleware order: `Router::layer` wraps bottom-to-top, `tower::ServiceBuilder` runs top-to-bottom; prefer one
  `ServiceBuilder` per router so the order reads as written. Layers only apply to routes added before `.layer(..)`.
- tower-http layers in use: `CompressionLayer::new()` on `/v1/systemone` and `/v1/models` only (features
  `compression-br`, `compression-gzip`): `Router::layer` wraps only the routes added before it, so the browser route
  is added after it and its files go out as stored, with their length (the default predicate would compress
  `application/octet-stream` and JSON and drop `Content-Length`). Do not stack `CompressionLayer` over the
  memory-serve router; memory-serve already serves brotli/gzip variants itself. No `TraceLayer` yet: it only emits `tracing` events, and the binary installs no
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
  loading or enqueueing when `queued + plan.rows > max_queued_rows`, and decrement when the worker has decoded the
  job or the job is discarded: the job owns the guard (its `Drop`), so a client that goes away leaves its queued rows
  counted. Treat `TrySendError::Full` as the same 503.
- Use `tokio::task::spawn_blocking` only for bounded one-off work (model load, `hf`/registry file reads). Its tasks
  cannot be aborted once running, and runtime shutdown waits for them.
- LRU eviction and the `--keep-alive` idle unload close the worker's channel; the worker drops the model after its
  queue drains, so eviction never cancels an admitted request. Each model has a turn (a fair `tokio::sync::Mutex`)
  that a request holds from looking up the worker, loading it if needed, to queueing its job, so one model's requests
  queue in arrival order and it loads once; the loaded-set lock is never held across a load, so `/health` and other
  models answer meanwhile. At most `max_loaded_models` models hold weights, counting models still loading and evicted
  ones still draining: a load first reserves a place (unloading the least recently used listed model when every
  place is listed) and waits until a model drops its weights.
  A request holds a lease (in-flight count, last use) from acquiring the worker to its answer; one tokio task per
  loaded model sleeps until `last use + keep_alive` and unloads the model only when it holds no lease, keeping only
  `Weak` references so it never keeps a model alive.

## Registry and library pulls
- `Models` keeps the registry as `models.toml` last read and rereads it whenever the file's modification time
  changes (`Registry::modified`), on every request, `/v1/models` and `/health`; a model `ardana pull` adds, changes or
  removes while `serve` runs takes effect without a restart. A reader (`Decider`) or a loaded worker built from an
  older entry of the same name is rebuilt: both remember the `ResolvedModel` they came from and are compared with the
  current entry.
- Model resolution: `jev-*` and no model mean the default (`--default-model`, else the first registry entry, else the
  library default); `Registry::named` then gives a pulled entry, a library model to pull, or the 404 listing pulled
  and library names.
- A library model that is not pulled is pulled by one detached `tokio::spawn`ed task per registry name
  (`models.rs#Pulls`, a `watch` channel per pull, shared with the browser variants' pulls; a library pull is forgotten
  once done, since the registry holds its result): every request for that name waits on the same outcome, and the pull
  finishes and is recorded even when its requesters go away. The pull itself runs on `spawn_blocking` with
  `Handle::block_on`, since it reads the tokenizer and chat template synchronously; the task rereads `models.toml`
  under a process-wide lock right before inserting and saving, so concurrent pulls never drop each other's entries.
  A failed pull answers its waiters 500 `api_error` and is retried by the next request.
- The server logs pulls on stderr (`ardana serve: pulling <name> (<ref>, <size>)`, byte progress of each download,
  `pulled <name> into <models.toml>`); there is still no tracing subscriber.
- `GET /v1/models` keeps Jev's `{name, description, release_date}` per entry and adds `x_pulled` (every entry),
  `x_default` (the default model only), `x_size` (library models not pulled yet, the GGUF's bytes), `x_browser`
  (a library model with a browser variant, pulled or not: the bytes a tab downloads, `library.toml`'s browser `size`)
  and `x_browser_pulled` (a browser variant the hub cache holds whole, so a request for its files pulls nothing; on a
  public server too). `Models` asks `ardana_registry::BrowserCache`, built once, which finds every file a pull reads
  in the cache without reading one or calling the Hub, so the list stays cheap; the playground says the server pulls
  only when it does not hold the variant.

## Browser variants (`/v1/browser`)
- `GET /v1/browser/{name}/{file}` serves the browser variant of the library model `name` (exactly a library name with
  a `[model.browser]` table): `model.onnx`, `model.onnx.data` and `tokenizer.json` from the server's hub cache, and
  `profile`, the `ardana_core::ModelProfile` the registry derives for the variant (`ardana_registry::pull_browser`:
  `decider_config.json` beside the files, else the stock profile in the library's layout), as JSON. Every other name
  or file, a `name:quant`, a path a percent-encoded `/` or `..` builds and a path segment that is not UTF-8 (the `Path`
  rejection) are the API's 404 before anything is pulled; another method is its 405. No path or URL comes from the
  request: the files are the ones the pull returned.
- The first request for a model pulls its variant, cache first: a variant whose files are all in the hub cache is read
  in place without a Hub call (as `HF_HUB_OFFLINE` reads a repository), so the server pulls it once; otherwise the
  repository is pulled at its `main` commit (unless `HF_HUB_OFFLINE` is set). Cache first keeps a page load off the
  Hub, and lets the in-process tests and the offline e2e servers read `tmp/hf` without the variable. Requests for one
  model share one pull (`models.rs#Pulls`, the library pulls' mechanism, kept in `Models`; `browser.rs#pull` is the pull
  itself), on `spawn_blocking` since it reads the tokenizer; the pulled variant is kept while the server runs, a failed
  pull answers 500 `api_error`
  and is retried by the next request, and a file gone from the cache answers 500 and makes the next request pull
  again. The server logs `ardana serve: pulling the browser variant of <name> (<ref>, <size>)` and the download
  progress of each file, then `serving the browser variant of <name> (<ref> at <commit>)`.
- A file goes out as stored: a body that reads the file in 1 MiB frames as the client takes them (`FileBody`, on
  `tokio::fs::File`) with an exact `SizeHint` and an explicit `Content-Length`, `Content-Type`
  `application/octet-stream` (`application/json` for `tokenizer.json`), never compressed. Every browser response
  carries `ETag: "<snapshot commit>"`, which the tab keys its copy of the files by, and `Cache-Control: no-store`: the
  tab keeps the files in its own Cache Storage, so the HTTP cache need not hold a second copy.
- The three files go out by byte range too (RFC 9110), which a tab resumes a stopped download with: every file
  response says `Accept-Ranges: bytes`; one range in `Range: bytes=` (`a-b`, `a-`, `-n`) is its part, `206` with
  `Content-Range: bytes a-b/<length>` and the part's `Content-Length` (the file seeked, `FileBody` sized to the part);
  an `If-Range` that is not the file's `ETag` (another version, a weak tag, a date: no file has a `Last-Modified`),
  several ranges, another unit or a range that does not parse are the whole file (`200`); a range that starts at or
  past the end is `416` with `Content-Range: bytes */<length>` and `{"detail":"Range Not Satisfiable"}`
  (`ApiError::RangeNotSatisfiable`). The profile is always whole (`browser.rs#part` reads the headers).

## Public mode (`--public`)
- `ardana serve --public` (`ARDANA_PUBLIC=1`) serves a public playground whose visitors run browser models in their
  own tabs; the server runs no model. The binary gives it no runtime (`Runtimes(Vec::new())`) and no registry: it opens
  no `models.toml` (`Registry::default()`, empty and backed by no file), so an unreadable one cannot stop it, and
  `Models` with `ModelOptions::public` never reads or writes a registry, never resolves `--default-model` and
  pulls only browser variants: `Models::resolve` answers `ApiError::RunsNoModel` first, `GET /v1/models` lists the
  library alone (every model `x_pulled: false`, with `x_size`, `x_browser` and `x_browser_default`, none `x_default`),
  `GET /health` is `{"status":"ok"}`.
- The router routes `POST /v1/systemone` to `refuse`, a handler with no body extractor: every decision is the 403
  before a byte of its body is read (no 422, no 413, no 32 MiB buffered), and `Models::resolve` refuses too, for
  in-process callers. Every other route behaves as on a local server: the same 404s and 405s, the Q13 headers, no CORS
  header (even for a request with an `Origin` or a preflight).
- A 5xx of a public server says what failed, never why: `browser.rs#file` answers a failed pull or a lost file with
  "the browser files of <name> are not available on this server; try again later" (`name` is a library name by then)
  and logs the reason, which can name the hub cache path or the `hf.co/` reference, on stderr. A local server keeps
  the reason in the body.
- The flags of the model lifecycle (`--default-model`, `--keep-alive`, the limits) do nothing in public mode, and
  `--api-key` gates `/v1` as it does locally (the playground sends no key).

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
- memory-serve sets one `Cache-Control` for HTML and one for every other file (its default week,
  `max-age=604800, stale-while-revalidate=86400`), which suits trunk's content-hashed names and the files whose names
  change with their content (the fonts, onnxruntime-web's versioned directory). The in-tab engine's directory keeps its
  names from build to build (`/engine/`: the engine module and `engine.js`, `leptos.md`), so `lib.rs#revalidate`, a
  `middleware::from_fn` layer on the playground router, sends its files with `Cache-Control: no-cache`: the browser
  asks again on every load and memory-serve answers `If-None-Match` with 304 while the file is unchanged.
- `ardana-server` depends on memory-serve with `force-embed`, so debug builds embed too and every binary serves from
  memory, never from the source tree (R6.3). `crates/ardana-server/tests/playground.rs` checks that `/`, unknown
  paths and every asset come from the directory `build.rs` embedded, and each file's `Cache-Control` (`no-cache` and a
  304 on revalidation for the page and `/engine/`, the week for the rest) on a local and a public server.

## Testing
- Test handlers in `crates/ardana-server/tests/*.rs` without binding a port: build the app with a fake `Runtime`
  and call `app.clone().oneshot(Request::builder()...body(Body::empty())?)` from `tower::ServiceExt`; read bodies
  with `axum::body::to_bytes(body, usize::MAX)`. The fake runtime (`tests/common/mod.rs`) counts loads and drops and
  can hold loads and decodes; requests are read with decider-2b's real tokenizer and profile from `tmp/hf`.
- Assert status, headers (`Retry-After`, `Content-Type`) and the exact JSON shape of every error body, including
  that no bad-input case returns 5xx.
- `tests/browser.rs`: `browser_routes` (plain) checks `x_browser` in `/v1/models`, the 404s and 405s of `/v1/browser`
  and the Q13 headers on every kind of response; `public_mode` (plain: it pulls nothing) builds `Models` over an empty
  home with the fake runtime and `public`, and checks the listing, the 403 for every body (malformed and over the limit
  among them), `/health`, traversal as sent and percent-encoded, unknown names and other files (404), other methods and
  a preflight (405), bodies that name no path or URL, no CORS header for a request from another origin, no load, and
  the home still empty; `browser_files` (`#[ignore]`d: it reads the ONNX repositories
  `cargo xtask onnx convert` writes into `tmp/hf`, which CI does not fetch) checks one shared pull, the three files
  byte for byte, uncompressed with their length under `Accept-Encoding: br`, the `ETag`, byte ranges of the graph and
  the weights (each part byte for byte, `If-Range`, the whole file for another version or several ranges, the 416) and
  the profiles of decider-0.8b and qwen3.5-0.8b. `browser.rs`'s unit test reads every kind of `Range` and `If-Range`
  without a file.
- Fake runtimes are for lifecycle and error-path tests only; end-to-end checks (`cargo xtask e2e jevcompat|sdk|
  jevbench`) run the release binary on decider-2b pulled offline from `tmp/hf`, each in its own `ARDANA_HOME` under
  `tmp/e2e/<suite>`, on a free port of 127.0.0.1, stopped with SIGTERM afterwards. `cargo xtask e2e public` probes
  the release `ardana serve --public` with curl (`--path-as-is`, so `..` reaches the server as written) and, over an
  empty Hub cache, a failed pull's wording (`playwright.md`); `crates/ardana/tests/serve.rs` starts the debug binary
  with `ARDANA_PUBLIC=1`, and with `--public` over an `ARDANA_HOME` whose `models.toml` does not parse, which it serves
  and leaves as it was.

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
- https://docs.rs/axum/0.8/axum/middleware/fn.map_response.html — `map_response`, the isolation headers on every response
- https://docs.rs/axum/0.8/axum/body/index.html — `HttpBody`, http-body's trait a streaming file body implements
- https://docs.rs/tokio/latest/tokio/fs/struct.File.html — `tokio::fs::File`, `AsyncRead::poll_read` off the async workers
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
