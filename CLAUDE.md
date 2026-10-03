# Ardana

Ardana is a local-first tool that pulls and runs open System 1 decision models (decider-style one-pass readouts) the
way Ollama does, from one `ardana` Rust binary that serves them over an HTTP API (compatible with Jev and TypeSafe
clients) and bundles a Rust/WASM playground. llama.cpp is the first runtime behind the `Runtime` abstraction; Qwen3.5
is the first model family. Planning documents live in gitignored `*-prd/` folders; when one is present locally, its
Decisions and Contracts are binding (as its Amendments change them) and its Requirements are the definition of done.

## Workspace

| Crate | Role | May depend on (workspace crates) |
|-------|------|----------------------------------|
| `crates/ardana` | binary, wires runtimes into registry and server | any |
| `crates/ardana-api` | Jev wire types and error bodies, compiles for `wasm32-unknown-unknown` | none |
| `crates/ardana-core` | prompts, tokenizer, readout, runtime traits | `ardana-api` |
| `crates/ardana-llama` | llama.cpp runtime | `ardana-core` |
| `crates/ardana-registry` | refs, the model library, `models.toml`, HF and Ollama resolution | `ardana-core` |
| `crates/ardana-server` | axum server, model lifecycle, embedded playground | anything but `ardana-llama` |
| `crates/ardana-playground` | Leptos CSR playground built by trunk, running browser models in the tab (onnxruntime-web) | `ardana-api` |
| `crates/ardana-engine` | the in-tab engine's tokenizer, planner and readout (ardana-core), a second wasm module | `ardana-api`, `ardana-core` |
| `xtask` | task runner | any |

`cargo xtask check-deps` enforces the direction.

## Commands

- `cargo xtask env` prints the sandbox environment as shell exports; `cargo xtask env --claude` writes it (all but
  `HOME`) into `.claude/settings.local.json` `env`.
- `cargo xtask fetch` installs every `xtask/fetch.toml` entry into `tmp/` (tools, research clones in `tmp/src`
  (decider, jevcompat, JevBench, llama.cpp b11074), the jevcompat uv tool, the `tmp/py/sdk` and `tmp/py/jevbench`
  venvs, Hub models in `tmp/hf/hub`, `[[hf_local]]` copies of gated repos from the real `~/.cache/huggingface/hub`);
  `cargo xtask fetch --check` verifies them. It also runs `npm ci` in `e2e/playground` (`@playwright/test`,
  `lz-string`, `@typesafe-ai/sdk`) and copies the impeccable binary to `tmp/impeccable/bin/0.1.5/impeccable`. Run it before `cargo test`: plain tests read the library models' (decider-2b,
  Qwen3.5, SmolLM3) tokenizers, configs and chat templates from `tmp/hf`. `cargo xtask fetch --tests` installs only
  those files (every `[[hf]]` file but the GGUF and ONNX weights, no token), which is what CI runs; the gated Llama 3.2
  tokenizer serves only the `#[ignore]`d real-model tests, next to Ollama's `llama3.2`.
- CI (`.github/workflows/ci.yml`) checks every pull request and push to `main`: `cargo fmt --check`, clippy with
  `-D warnings` and a wasm check of `ardana-api`/`ardana-playground`/`ardana-engine`, `check-deps`, `check-docs`,
  `dist generate --check` and `dist plan`, `cargo test --workspace` on Linux and macOS (after `fetch --tests`; no
  secrets), a Windows build and the release's trunk playground build. The real-model
  suites stay local. `rust-toolchain.toml` pins Rust 1.97.1 for local, CI and release builds.
- `cargo xtask build` builds the playground `dist/` with the fetched trunk, offline (the page's wasm, and the engine
  module and `engine.js` in `dist/engine/`), then the release `ardana`, which embeds it (memory-serve, `force-embed`)
  and serves it at `/`. Building `ardana-server` without a dist (or with `ARDANA_PLAYGROUND_DIST` at a directory
  without `index.html`) warns and embeds a placeholder page.
- Releases: dist 0.32.0 (`tmp/bin/dist`, from `cargo xtask fetch`) generates `.github/workflows/release.yml` from
  `dist-workspace.toml` and `.github/build-setup.yml` (the playground build each runner does first); after changing
  either run `tmp/bin/dist generate` (`generate --check` fails on drift). Releases start only by hand: the
  "Cut release" workflow (`.github/workflows/cut-release.yml`, patch/minor/major) bumps the workspace version, commits
  and tags it on `main` and dispatches `release.yml`, which builds `ardana` alone for aarch64/x86_64 macOS,
  aarch64/x86_64 Linux and x86_64 Windows with `ardana-installer.sh` and `ardana-installer.ps1` (into `~/.local/bin`)
  and publishes the GitHub Release; `release.yml` run alone (tag `dry-run`) builds without publishing, pull requests
  run nothing. `cleanup-artifacts.yml` deletes each finished Release run's artifacts; any other workflow sets
  `retention-days: 1` on its uploads. Locally: `tmp/bin/dist plan`,
  `tmp/bin/dist build --artifacts=local --target aarch64-apple-darwin` and `tmp/bin/dist build --artifacts=global`
  (output in `target/distrib/`). Nothing is pushed and no workflow runs from an agent session.
- `cargo xtask check-deps`, `cargo xtask check-docs` check the workspace shape and these documents.
- `cargo xtask export-decider` regenerates the vendored decider prompt goldens in
  `crates/ardana-core/tests/data/decider/` by running decider@23579f7 from `tmp/src/decider`.
- `cargo xtask onnx convert <name>` builds a library model's browser variant from the checkpoint its `library.toml`
  entry names as `source`: the int4 or int8 ONNX (onnxruntime-genai 0.17.1 model builder in the `tmp/py/onnx` venv it
  creates) and, when the entry's `weights` is an ardana-ai repository (decider-0.8b), the GGUF (llama.cpp's
  `convert_hf_to_gguf.py` from `tmp/src/llama.cpp`). It writes `hf.co/ardana-ai/<name>-ONNX` (and `-GGUF`) into
  `tmp/hf/hub` as local snapshots, records their sizes in `library.toml` and reads them back offline with the release
  `ardana pull`; a model without a `[model.browser]` table is refused. `cargo xtask onnx publish <name> [--dry-run]`
  prints the model cards, the `hf repos create` and `hf upload` commands and the `[[hf]]` entries, then uploads the
  snapshots to huggingface.co/ardana-ai with the user's token (`HF_TOKEN`, else the real `~/.cache/huggingface/token`)
  and pins them in `xtask/fetch.toml`; a failed upload exits non-zero and changes nothing. Both run by hand only,
  never in CI (`docs/guidelines/onnx.md`).
- `cargo xtask e2e <suite>` runs an end-to-end suite: `smoke`, `rust` (`cargo test --workspace --
  --include-ignored`, the real-model tests included), or one of the API suites against a release `ardana serve` on
  decider-2b: `jevcompat` (jevcompat 0.1.0, open and with a key, every MUST must pass), `sdk` (typesafe-sdk 0.7.2
  parses `ticket.json`) and `jevbench` (231 public items, zero failed requests; output in `tmp/evals/jevbench/`); the browser suites
  `playground` (Playwright on the installed Chrome against the built binary copied alone into `tmp/`, plus the
  placeholder build; the W6 cases, then the builder, state modes, the 17 docs.typesafe.ai share links in
  `e2e/playground/fixtures/jev-share-links.json`, the share round trip, the presets, the executed snippets (the
  ardana commands of an "In browser" row, `ardana pull` then `ardana run`, run with the binary under test),
  `run_command` (every model the server has not pulled, under any spelling, shows `ardana pull <name>` and `ardana
  run <name> --request -` and no install line, and Run sends nothing, autorun included), `pull_while_open` against
  one more empty server (the shown `ardana pull qwen3.5-0.8b` run with the binary under test, the tab's return makes
  the row a server row, and Run answers from the server), `first_run`, `browser_stop` and `browser_recover` against
  one more server on an empty registry
  (the picker opens on decider-0.8b's "In browser" row and the first Run answers in the tab; an autorun link waits for
  the tap, Stop and another row end a download; a dropped connection, then Run answers without a reload; a runtime or
  an engine module that did not load is imported again under a new query, and WebGPU answers again),
  `banner_focus` (no control takes the focus under the sticky banner while a download is in flight, at 320 to 1280 px
  and with 200% text), the reflow cases (`banner_reflow`: in flight at 400% zoom, with 200% text on a phone and in
  landscape, and both at once, what stays of the bars holds a third of the viewport, Stop follows Run, no focus is
  hidden; `text_reflow`: 200% text from 280 to 480 px runs nothing past the edge; `term_tooltips`: a fact's tooltip
  never covers the in-flight banner), the focus cases (`skip_link`, `share_popover`, `announcements`, `run_focus`,
  `aria_relationships`, `term_stops`, `page_focus`: the skip link without a fragment and Restore previous after a
  share link, the popover's focus-out and Escape, repeated outcomes and presets said, a phone run's focus, no dangling
  relationship, one stop per fact row and no tooltip in a name, focus after Add and Stop), `touch_targets` (44px reach
  under a finger), `current_row` (the current section's weight in every scheme) and `page_weight` (no math or symbols
  font, one logo drawing), and `browser_run`, `insecure_origin` and `browser_resume` against one more empty server
  (decider-0.8b's "In browser" row runs the ticket preset in the tab: no pull from a server that holds the files, at
  1 s and 600 ms of latency, the download's bytes as one stage, the stages said once to screen readers, the server's
  top answers on WebGPU and WASM, a server row picked mid-row releasing the session once that row is over, a reload
  that downloads under 1 MB, the server's 422 body; on a page that is no secure context, on WASM, keeping nothing; and
  through a proxy that drops the weights' connection, then after Stop, then after a reload, each next run resuming with
  `Range: bytes=<had>-` and a 206, the weights crossing the wire about once), `browser_pull` against one more server
  over an empty Hub cache (the first run in the tab says the server pulls, then that the server could not provide the
  files); `@design` and `@public` tests left out;
  screenshots in `tmp/screens/<case>/`, reports in `tmp/playwright/`), `public` (`cargo xtask e2e public`: the release
  `ardana serve --public` on an empty `ARDANA_HOME`, offline over `tmp/hf`; curl probes its HTTP surface with every
  path sent as written: the library listed, `POST /v1/systemone` 403, `/health` naming no model, `..` and
  percent-encoded traversal, unknown names and other files 404, other methods 405, no `Access-Control-*` header and no
  path or URL in a body; the `@public` Playwright case `public_playground` at both viewports, where every server row
  runs with the ardana CLI on the visitor's machine, after ardana.ai's install line, and "Run decider-0.8b in this tab
  instead" answers in the tab; a second public server
  over an empty Hub cache, whose failed pull answers without its reason; both homes still empty afterwards) and
  `design` (/impeccable context, then `impeccable detect` of the empty, loaded, results, 422 and `run-command` states
  and the `public` playground at 1280x800 and 390x844, and of the in-tab `browser-download` and `browser-results`
  states the `@design` Playwright test freezes from a real run into `tmp/evals/design/<state>-<viewport>.html`,
  reports in `tmp/evals/design/`, then the finish: a `.impeccable/critique/` record, `docs/design/audit.md` with
  `P0: 0 · P1: 0`, clean scans, the hook enabled).
- `ardana pull <name|ref> [--tokenizer hf.co/<org>/<repo>|<path>] [--name N] [--layout plain|chat]` records a model
  in `$ARDANA_HOME/models.toml` (default `~/.ardana`; `tmp/ardana` under cargo), printing download progress on
  stderr. A library name `<name>[:<quant>]` (`crates/ardana-registry/src/library.toml`: `decider-2b`, `decider-0.8b`,
  `decider-4b`, `qwen3.5-0.8b`, `smollm3-3b`; the default model is `decider-2b`; decider-0.8b, decider-2b and
  qwen3.5-0.8b also have a browser variant, `browser_default` decider-0.8b) stands for its `hf.co/` repository and is recorded
  under that name (`decider-2b:q8_0` for another quant, matched case-insensitively); refs are
  `hf.co/<org>/<repo>[:<quant>]` (default Q4_K_M), `hf.co/<org>/<repo>:<file>.gguf`, `ollama:[<ns>/]<name>[:<tag>]`
  (read in place from `$OLLAMA_MODELS`) and local GGUF paths. Downloads go to the standard HF cache (`$HF_HOME/hub`);
  `HF_HUB_OFFLINE=1` resolves `hf.co/` refs from the hub cache only. `ardana list` (`ls`: name, size, pull date,
  source), `ardana show <name> [--json]` (Model, Calibration and Files sections; a library model not pulled yet says
  how to get it) and `ardana rm <name>...` (the entries only, never model files; every name is checked first) manage
  it; `ardana ps [--host] [--port]` lists the models a running `serve` has loaded (from `/health`).
- `ardana run <name> [STATE] [--noul Q] [--choice Q OPTION...] [--score Q LEVEL...]` asks inline questions (`q1`,
  `q2`, ... in the order given; the state is read from stdin when left out), or `ardana run <name> --request
  <file|->` sends a whole `/v1/systemone` body. It prints each answer under its question (the answer marked `*`,
  figures in the playground's formats), `--json` prints the response instead and `--verbose` adds the model, tokens
  and timings on stderr; a library model not pulled yet is pulled first. `ardana run --gguf <file> --tokenizer
  <tokenizer.json> [--config <decider_config.json>] [--layout plain|chat] --request <file>` runs explicit files;
  both take `[--gpu-layers -1|0] [--n-ctx N]`. `--layout chat` reads the chat template next to the tokenizer, and
  without `--config` the model gets the stock profile.
- `ardana serve` (alias `start`) `[--host 127.0.0.1] [--port 8000] [--api-key K] [--default-model NAME]
  [--keep-alive 5m] [--max-loaded-models 1] [--max-queued-rows 4096] [--public]`, each flag also read from
  `ARDANA_<FLAG>` (`ARDANA_PORT`, `ARDANA_API_KEY`, `ARDANA_PUBLIC=1`, ...), serves the registry's models and the
  library's: `POST /v1/systemone`, `GET /v1/models` (pulled models, then library models with `x_pulled: false` and
  `x_size`; `x_default` marks the default, `x_browser` the bytes of a browser variant, `x_browser_pulled` a browser
  variant the hub cache holds whole (public mode too), `x_browser_default` the library's `browser_default`),
  `GET /health`, and
  `GET /v1/browser/<name>/{model.onnx,model.onnx.data,tokenizer.json,profile}` (a library model's browser variant
  from the hub cache, pulled once on its first request, cache first; the three files also by byte range,
  `Accept-Ranges: bytes`, a 206 for one range, `If-Range` on the `ETag`, a 416 past the end, so a tab resumes a
  download that stopped; the playground's "In browser" rows run it in the tab with the engine module
  (`crates/ardana-engine`, at `/engine/`, imported on the pick) and onnxruntime-web, vendored in
  `crates/ardana-playground/ort/`). `/engine/` goes out with `Cache-Control: no-cache` (its names never change),
  every other asset with memory-serve's week. Every response carries COOP
  `same-origin`, COEP `require-corp` and CORP `same-origin`. A request naming a library model that is not pulled pulls it first
  (concurrent requests share the pull); a request without a model, or with the compatibility alias `jev-latest` or any
  `jev-*` name, uses the default model: `--default-model`, else the first pulled model, else `decider-2b`. The
  playground speaks only real model names, opens on the default model when it is pulled and else on decider-0.8b's
  "In browser" row, and never makes the server pull (under any spelling of a name): a model the server has not
  pulled shows `ardana pull <name>`, to run where the server runs, and `ardana run <name> --request -` with the exact
  request instead, as the snippets do for an "In browser" row (the pull only while the server lacks the model); the
  page lists the models again after each run and whenever its tab comes back, so a model pulled meanwhile runs on the
  server. On `--public` a server row shows ardana.ai's install line in the pull's place and offers the browser default
  in the tab instead.
  `models.toml` is reread when it changes, so a model `ardana pull` adds while `serve` runs is servable at once;
  models load on their first request and unload after `--keep-alive` idle. `ardana serve --public` is a public
  playground that runs no model: no runtime, no registry (no `models.toml` opened), the library listed with no
  default, `POST /v1/systemone` 403 (`permission_error`) before its body is read, `/health` naming no model, the
  browser files served (a failure answered without its reason, which goes to the log), so its visitors run browser
  models in their own tabs.
- Running Ardana by hand: under `cargo run` (and `cargo test`), `.cargo/config.toml` puts `ARDANA_HOME` in
  `tmp/ardana` and `HF_HOME` in `tmp/hf`, so `cargo xtask build` then `cargo run --release -p ardana -- serve` serves
  the sandbox: its playground opens on decider-0.8b's "In browser" row while nothing is pulled, and an API request
  pulls decider-2b (from `tmp/hf` when `cargo xtask fetch` has put it there).
  The release binary run directly (`target/release/ardana serve`) uses `~/.ardana` and the standard HF cache
  (`$HF_HOME`, else `~/.cache/huggingface`), like any installed tool; do not do that from an agent session.
- Before reporting work: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace`, `cargo test --workspace`, plus the item's Verify command.

## Sandbox

- Every download, cache, model, tool, venv, clone and temp file of builds, tests, evals and agent commands stays in
  the gitignored `tmp/` at the repo root (plus the gitignored `e2e/playground/node_modules`); `rm -rf tmp
  e2e/playground/node_modules` fully resets. Nothing is written elsewhere under the home directory.
- `.cargo/config.toml` `[env]` forces `HF_HOME`, `ARDANA_HOME`, `ARDANA_TMP` and `TMPDIR` into `tmp/` for every
  cargo-run process, including tests.
- xtask runs every tool through `Sandbox` (`xtask/src/sandbox.rs`) with `HOME=tmp/home`, every tool cache in `tmp/`
  and `tmp/bin` first on `PATH`. `cargo xtask fetch`, `build`, `onnx` and every `e2e` suite run under the home guard, which
  fails the step when anything changes under the guarded home paths (`~/.cache/huggingface`, `~/.cargo/bin`, ...).
- Commands you run yourself: `cargo xtask env --claude` has put the tool variables into this session's environment;
  in a shell without them run `eval "$(cargo xtask env)"` in the same command before uv, pip, npm, npx, node, python,
  hf, trunk or impeccable. Never run those with default caches. Clone research repositories into `tmp/src/<name>`.
- Tools come from `xtask/fetch.toml` through `cargo xtask fetch`; never `cargo install` into `~/.cargo/bin`, never use
  the user's own `~/.cargo/bin/trunk`. The allowed exceptions are Cargo's own home (`~/.cargo` registry and git) for
  workspace builds, rustup, the installed Chrome app's own caches and macOS system caches.

## Git

- Git is local only: there is no remote. Never `git remote add`, `git push`, open pull requests or wait for CI.
- Work happens on feature branches off `main` with plain commits, at least one per work item; merging into `main`
  stays with the user. What a PR body would carry goes into the final report instead.

## UI work

- All playground UI work goes through the /impeccable skill (Q23, Q27): `PRODUCT.md`, `DESIGN.md` and the playground
  surface brief come first, the design hook stays on, and styles live in `.css` files. Run every /impeccable step in
  the sandbox environment.
- Browser validation (Q24): Playwright suites on the installed Chrome, `impeccable detect` URL scans at 1280x800 and
  390x844, and screenshots of every case under `tmp/screens/`.

## End-to-end verification

- Every work item is verified end to end on real open models: official Hugging Face sources
  (Mapika/decider-2b-GGUF Q4_K_M, ggml-org/Qwen3.5-0.8B-GGUF, ggml-org/SmolLM3-3B-GGUF) or the local Ollama store
  (`llama3.2`), fetched into `tmp/hf` by `cargo xtask fetch`. Never replace a real model with an invented fixture, never
  assert against a mock for an end-to-end requirement, never report a check you did not run.

## Guidelines

Read the guideline for every technology you touch and follow it. When a change alters how the stack is used (a new
pinned version, library, pattern or rule), update the matching guideline in the same commit.

- [Rust, cargo workspace, errors, testing, xtask](docs/guidelines/rust.md)
- [llama-cpp-2 and llama.cpp](docs/guidelines/llama-cpp.md)
- [Hugging Face tokenizers, hf-hub, cache layout, chat templates](docs/guidelines/huggingface.md)
- [axum and tower-http](docs/guidelines/axum.md)
- [Leptos CSR, trunk, wasm, CSS](docs/guidelines/leptos.md)
- [Playwright end-to-end tests](docs/guidelines/playwright.md)
- [Python tooling: uv venvs and tools](docs/guidelines/python-tooling.md)
- [ONNX browser variants: the model builder, the ardana-ai repositories, onnxruntime-web](docs/guidelines/onnx.md)
