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
| `crates/ardana-api` | Jev wire types, compiles for `wasm32-unknown-unknown` | none |
| `crates/ardana-core` | prompts, tokenizer, readout, runtime traits | `ardana-api` |
| `crates/ardana-llama` | llama.cpp runtime | `ardana-core` |
| `crates/ardana-registry` | refs, the model library, `models.toml`, HF and Ollama resolution | `ardana-core` |
| `crates/ardana-server` | axum server, model lifecycle, embedded playground | anything but `ardana-llama` |
| `crates/ardana-playground` | Leptos CSR playground built by trunk | `ardana-api` |
| `xtask` | task runner | any |

`cargo xtask check-deps` enforces the direction.

## Commands

- `cargo xtask env` prints the sandbox environment as shell exports; `cargo xtask env --claude` writes it (all but
  `HOME`) into `.claude/settings.local.json` `env`.
- `cargo xtask fetch` installs every `xtask/fetch.toml` entry into `tmp/` (tools, research clones in `tmp/src`
  (decider, jevcompat, JevBench), the jevcompat uv tool, the `tmp/py/sdk` and `tmp/py/jevbench` venvs, Hub models in
  `tmp/hf/hub`, `[[hf_local]]` copies of gated repos from the real `~/.cache/huggingface/hub`);
  `cargo xtask fetch --check` verifies them. It also runs `npm ci` in `e2e/playground` (`@playwright/test`,
  `lz-string`, `@typesafe-ai/sdk`) and copies the impeccable binary to `tmp/impeccable/bin/0.1.5/impeccable`. Run it before `cargo test`: tests read the decider-2b, Qwen3.5,
  SmolLM3 and Llama 3.2 tokenizers and chat templates from `tmp/hf`.
- `cargo xtask build` builds the playground `dist/` with the fetched trunk, offline, then the release `ardana`, which
  embeds it (memory-serve, `force-embed`) and serves it at `/`. Building `ardana-server` without a dist (or with
  `ARDANA_PLAYGROUND_DIST` at a directory without `index.html`) warns and embeds a placeholder page.
- `cargo xtask check-deps`, `cargo xtask check-docs` check the workspace shape and these documents.
- `cargo xtask export-decider` regenerates the vendored decider prompt goldens in
  `crates/ardana-core/tests/data/decider/` by running decider@23579f7 from `tmp/src/decider`.
- `cargo xtask e2e <suite>` runs an end-to-end suite: `smoke`, `rust` (`cargo test --workspace --
  --include-ignored`, the real-model tests included), or one of the API suites against a release `ardana serve` on
  decider-2b: `jevcompat` (jevcompat 0.1.0, open and with a key, every MUST must pass), `sdk` (typesafe-sdk 0.7.2
  parses `ticket.json`) and `jevbench` (231 public items, zero failed requests; output in `tmp/evals/jevbench/`); the browser suites
  `playground` (Playwright on the installed Chrome against the built binary copied alone into `tmp/`, plus the
  placeholder build; the W6 cases, then the builder, state modes, the 17 docs.typesafe.ai share links in
  `e2e/playground/fixtures/jev-share-links.json`, the share round trip, the presets, the executed snippets, and
  `first_run_pull` against one more server per viewport on an empty registry (RUN pulls decider-2b offline);
  screenshots in `tmp/screens/<case>/`, reports in `tmp/playwright/`) and `design` (/impeccable context, then
  `impeccable detect` of the empty, loaded, results and 422 states at 1280x800 and 390x844, reports in
  `tmp/evals/design/`, then the finish: a `.impeccable/critique/` record, `docs/design/audit.md` with
  `P0: 0 · P1: 0`, clean scans, the hook enabled).
- `ardana pull <name|ref> [--tokenizer hf.co/<org>/<repo>|<path>] [--name N] [--layout plain|chat]` records a model
  in `$ARDANA_HOME/models.toml` (default `~/.ardana`; `tmp/ardana` under cargo), printing download progress on
  stderr. A library name `<name>[:<quant>]` (`crates/ardana-registry/src/library.toml`: `decider-2b`, `decider-4b`,
  `qwen3.5-0.8b`, `smollm3-3b`; the default model is `decider-2b`) stands for its `hf.co/` repository and is recorded
  under that name (`decider-2b:q8_0` for another quant, matched case-insensitively); refs are
  `hf.co/<org>/<repo>[:<quant>]` (default Q4_K_M), `hf.co/<org>/<repo>:<file>.gguf`, `ollama:[<ns>/]<name>[:<tag>]`
  (read in place from `$OLLAMA_MODELS`) and local GGUF paths. Downloads go to the standard HF cache (`$HF_HOME/hub`);
  `HF_HUB_OFFLINE=1` resolves `hf.co/` refs from the hub cache only. `ardana list`, `ardana show <name>` (JSON) and
  `ardana rm <name>` (the entry only, never model files) manage it.
- `ardana run <name> --request <file>` answers with a registry model, pulling a library model first when it is not
  pulled yet; `ardana run --gguf <file> --tokenizer <tokenizer.json> [--config <decider_config.json>] [--layout
  plain|chat] --request <file>` with explicit files; both take `[--gpu-layers -1|0] [--n-ctx N]` and print one
  `/v1/systemone` response as JSON. `--layout chat` reads the chat template next to the tokenizer, and without
  `--config` the model gets the stock profile.
- `ardana serve [--host 127.0.0.1] [--port 8000] [--api-key K | ARDANA_API_KEY=K] [--default-model NAME]
  [--keep-alive 5m] [--max-loaded-models 1] [--max-queued-rows 4096]` serves the registry's models and the library's:
  `POST /v1/systemone`, `GET /v1/models` (pulled models, then library models with `x_pulled: false` and `x_size`;
  `x_default` marks the default), `GET /health`. A request naming a library model that is not pulled pulls it first
  (concurrent requests share the pull); a request without a model, or with the compatibility alias `jev-latest` or any
  `jev-*` name, uses the default model: `--default-model`, else the first pulled model, else `decider-2b`. The
  playground speaks only real model names. `models.toml` is reread when it changes, so a model `ardana pull` adds while
  `serve` runs is servable at once; models load on their first request and unload after `--keep-alive` idle.
- Running Ardana by hand: under `cargo run` (and `cargo test`), `.cargo/config.toml` puts `ARDANA_HOME` in
  `tmp/ardana` and `HF_HOME` in `tmp/hf`, so `cargo xtask build` then `cargo run --release -p ardana -- serve` pulls
  decider-2b into the sandbox on the playground's first RUN (from `tmp/hf` when `cargo xtask fetch` has put it there).
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
  and `tmp/bin` first on `PATH`. `cargo xtask fetch`, `build` and every `e2e` suite run under the home guard, which
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
  surface brief come first, the design hook stays on, styles live in `.css` files, and the finished playground passes
  a fresh-context critique and audit with every P0 and P1 finding fixed. Run every /impeccable step in the sandbox
  environment.
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
