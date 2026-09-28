# Ardana

Ardana serves System 1 decision models (decider-style one-pass readouts) over a Jev-compatible HTTP API from one
`ardana` Rust binary that bundles a Rust/WASM playground. llama.cpp is the first runtime behind the `Runtime`
abstraction; Qwen3.5 is the first model family. The Step 1 plan lives in `ardana-serve-prd/main.md`: its Decisions and
Contracts are binding, its Requirements are the definition of done.

## Workspace

| Crate | Role | May depend on (workspace crates) |
|-------|------|----------------------------------|
| `crates/ardana` | binary, wires runtimes into registry and server | any |
| `crates/ardana-api` | Jev wire types, compiles for `wasm32-unknown-unknown` | none |
| `crates/ardana-core` | prompts, tokenizer, readout, runtime traits | `ardana-api` |
| `crates/ardana-llama` | llama.cpp runtime | `ardana-core` |
| `crates/ardana-registry` | refs, `models.toml`, HF and Ollama resolution | `ardana-core` |
| `crates/ardana-server` | axum server, model lifecycle, embedded playground | anything but `ardana-llama` |
| `crates/ardana-playground` | Leptos CSR playground built by trunk | `ardana-api` |
| `xtask` | task runner | any |

`cargo xtask check-deps` enforces the direction.

## Commands

- `cargo xtask env` prints the sandbox environment as shell exports; `cargo xtask env --claude` writes it (all but
  `HOME`) into `.claude/settings.local.json` `env`.
- `cargo xtask fetch` installs every `xtask/fetch.toml` entry into `tmp/` (tools, research clones in `tmp/src`, Hub
  models in `tmp/hf/hub`); `cargo xtask fetch --check` verifies them. Run it before `cargo test`: tests read the
  decider-2b tokenizer from `tmp/hf`.
- `cargo xtask build` builds the playground `dist/` with the fetched trunk, offline.
- `cargo xtask check-deps`, `cargo xtask check-docs` check the workspace shape and these documents.
- `cargo xtask export-decider` regenerates the vendored decider prompt goldens in
  `crates/ardana-core/tests/data/decider/` by running decider@23579f7 from `tmp/src/decider`.
- `cargo xtask e2e <suite>` runs an end-to-end suite: `smoke`, or `rust` (`cargo test --workspace --
  --include-ignored`, the real-model tests included).
- `ardana run --gguf <file> --tokenizer <tokenizer.json> --config <decider_config.json> --request <file>
  [--gpu-layers -1|0] [--n-ctx N]` prints one `/v1/systemone` response as JSON.
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
