# Rust guidelines

Covers the Rust language, the cargo workspace, error handling, tests, lints, formatting and the `cargo xtask` task
runner for every Ardana crate: `crates/ardana` (binary), `crates/ardana-api`, `crates/ardana-core`,
`crates/ardana-llama`, `crates/ardana-registry`, `crates/ardana-server`, `crates/ardana-playground` and `xtask`.
Library-specific rules (llama-cpp-2, tokenizers, hf-hub, axum, leptos) live in their own guideline files.

## Versions
- `rustc` 1.97.1 — pinned by `rust-toolchain.toml` (with `clippy`, `rustfmt` and the `wasm32-unknown-unknown` target)
  for local, CI and release builds alike; every crate must build and test on it. Bump it there and here together.
- `edition` 2024 — set once in `[workspace.package]`; stabilized in Rust 1.85 and implies Cargo resolver 3.
- `resolver` 3 — set explicitly in the virtual root `[workspace]`, since a virtual manifest has no edition to infer it.
- `cargo-dist` 0.32.0 — the release tool (`dist`), installed into `tmp/bin` by `cargo xtask fetch`; the same version
  runs in `.github/workflows/release.yml` (`cargo-dist-version`).

## Rules

### Workspace and manifests
- Keep the root `Cargo.toml` a virtual manifest: `[workspace] members = [...]`, `resolver = "3"`, no root package.
- Declare `edition`, `rust-version`, `license` and `version` in `[workspace.package]`; members write
  `edition.workspace = true` (and the same for the other keys) instead of repeating values.
- Declare every third-party dependency once in `[workspace.dependencies]` with its pinned Q28 version
  (`llama-cpp-2 = "=0.1.157"`, `tokenizers = "0.23.2"`, `hf-hub = "1.0.0"`, `axum = "0.8"`, ...); members use
  `dep = { workspace = true }`. Add member-specific features at the member; they are additive to the workspace ones.
- Never mark a `[workspace.dependencies]` entry `optional`; make it optional in the member that needs it.
- Reference workspace crates by path in `[workspace.dependencies]` too (`ardana-core = { path = "crates/ardana-core" }`)
  so the dependency graph lives in one file that `cargo xtask check-deps` can read.
- Respect the dependency direction enforced by `cargo xtask check-deps`: `ardana-api` depends on no workspace crate,
  `ardana-core` only on `ardana-api`, `ardana-llama` and `ardana-registry` on `ardana-core`, `ardana-server` never on
  `ardana-llama`, `ardana-playground` only on `ardana-api`. Only the `ardana` binary wires `ardana-llama` into
  `Runtimes`. A new runtime is a new crate plus one registration line, never a server or playground change.
- Keep `ardana-api` and `ardana-playground` free of native-only dependencies; `ardana-api` must pass
  `cargo check -p ardana-api --target wasm32-unknown-unknown`.
- Current third-party pins: `anyhow` 1, `thiserror` 2 (library error enums), `indexmap` 2 with `serde` (ordered wire
  maps), `serde`/`serde_json` 1 with `preserve_order` (JSON objects keep insertion order, like Python dicts) and
  `float_roundtrip` (floats parse to the exact `f64` Python's `json.loads` gives, which prompts re-render), `clap` 4
  with `derive` (the `ardana` CLI), `sha2` 0.10 (test digests only), `toml` 1 (`models.toml`, `xtask/fetch.toml`),
  `tokio` 1 (the `ardana` binary runs `pull` on a current-thread runtime and `serve` on a multi-thread one;
  `ardana-server` adds `sync`, `signal`, `macros`), `axum` 0.8, `tower-http` 0.7 and `tower` 0.5 (dev) as
  `axum.md` pins them, plus the library pins in their own guidelines.
- Commit `Cargo.lock`: Ardana ships a binary, and the lockfile is also the source of the wasm-bindgen version that
  `xtask/fetch.toml` reads via `cargo-lock:wasm-bindgen`. Pass `--locked` in xtask steps that must not re-resolve.

### `.cargo/config.toml`
- Keep the alias `[alias] xtask = "run --quiet --package xtask --"` (quiet, so `eval "$(cargo xtask env)"` sees only
  the exports); do not add other aliases that hide real commands.
- Set `HF_HOME`, `ARDANA_HOME`, `ARDANA_TMP` and `TMPDIR` in `[env]` with `relative = true` (resolved against the
  directory holding `.cargo/`, i.e. the repo root) and `force = true` (overrides the shell). Cargo applies `[env]` to
  build scripts, rustc, `cargo run` and `cargo test`, which is what keeps plain `cargo test` inside `tmp/`.
- Set the llama.cpp build baseline in `[env]` without `force`: `GGML_AVX`, `GGML_AVX2`, `GGML_FMA`, `GGML_F16C`
  (`ON`), `LLAMA_STATIC_CRT` (`1`) and `CMAKE_MSVC_RUNTIME_LIBRARY` (the static MSVC runtime), so local and release
  builds configure llama.cpp alike (`llama-cpp.md`, "Build").
  The matching `+crt-static` for MSVC Rust code sits in `[target.'cfg(all(windows, target_env = "msvc"))'] rustflags`.
- Do not put `HOME`, `CARGO_HOME` or `RUSTUP_HOME` in `[env]`; Cargo's home and rustup are the allowed exceptions, and
  the full tool environment comes from `Sandbox::env` (`cargo xtask env`).

### Language
- Treat `std::env::set_var` and `remove_var` as the `unsafe` functions they are in edition 2024; never call them in
  library code or tests (tests run on parallel threads). Pass variables to children with `Command::env` or
  `Sandbox::command` instead.
- No `unwrap()`/`expect()` on values that come from requests, files, models or the network; reserve them for
  invariants and tests, and give `expect` a message that states the invariant.
- Bad client input must never panic: validate and return an error (the server maps errors to 4xx, never 5xx).
- Derive `Debug` on public types; derive `Clone`, `PartialEq`, `Eq`, `Default` where they make sense, because
  downstream crates cannot add them later.
- Keep `unsafe` confined to `ardana-llama` FFI edges (if any) with a `// SAFETY:` comment per block.
- `thiserror` treats a field named `source` as the error's source; name a field holding a model reference
  `reference` in error variants (`RegistryError`), even where the data type calls it `source` (`ResolvedModel`).

### Errors
- Library crates expose typed error enums callers can match on: `DecideError` (`Invalid { loc, msg }`,
  `Capacity`, `Runtime`), `ProfileError`, `ChatTemplateError`, `RegistryError`. Implement `std::error::Error` and
  `Display` for them (derive with `thiserror` from `[workspace.dependencies]`, or by hand); keep them `Send + Sync`.
- Write `Display` messages lowercase and without trailing punctuation, except where a message must reproduce
  decider's exact text (R2.3) or contain a contract substring ("context window", "has no chat template").
- Use `anyhow::Result` in `xtask`, in the `ardana` binary's command layer, and at the `Runtime`/`LoadedModel` trait
  boundary exactly as `crates/ardana-core/src/runtime.rs` defines it; wrap it as `DecideError::Runtime`.
- Attach context with `.context()`/`.with_context(|| ...)` naming the path, ref or model involved; use `bail!` and
  `ensure!` for early exits in xtask and the binary.
- Convert between error types with `From` impls (`#[from]`) so `?` works; do not stringify an error just to re-wrap it.

### Testing
- Unit tests live in a `#[cfg(test)] mod tests` next to the code and may test private items.
- Contract-level tests live in each crate's `tests/` directory, one file per concern with the names the plan fixes
  (`crates/ardana-core/tests/readout.rs` "decider_goldens", `xtask/tests/guard.rs` "detects_home_write", ...). Share
  helpers through `tests/common/mod.rs`, not a top-level `tests/common.rs`.
- Keep `main.rs` thin and logic in `lib.rs` so integration tests can call it; test the CLI through
  `env!("CARGO_BIN_EXE_ardana")`.
- The `ardana` CLI follows Ollama's shape (`serve`/`start`, `run`, `pull`, `list`/`ls`, `show`, `ps`, `rm`, `-v`):
  a one-line imperative description per command and flag, an `Examples:` block in every command's `after_help`
  (`crates/ardana/src/help.rs`, checked by a unit test), readable output by default with `--json` for the exact
  data, figures through `ardana_api::format`, and errors that end with the command to type next. Every `serve` flag
  also reads an `ARDANA_<FLAG>` variable, its value hidden in `--help`.
- Mark every test that needs a real model, network, the Ollama store or more than a few seconds with
  `#[ignore = "e2e: <what it needs>"]`. Plain `cargo test` stays fast and offline; `cargo xtask e2e rust` runs
  `cargo test --workspace -- --include-ignored` under the home guard.
- End-to-end tests use the official models from `xtask/fetch.toml` (decider-2b Q4_K_M, Qwen3.5-0.8B, SmolLM3-3B,
  Ollama `llama3.2`), resolved from `tmp/hf`; never replace a real model with an invented fixture.
- Put test temp files under `$ARDANA_TMP` (or `std::env::temp_dir()`, which `[env]` points at `tmp/sys`); never under
  the real home. Tests that run `ardana pull` give each child its own `ARDANA_HOME` under `$ARDANA_TMP/<test>` (tests
  run in parallel) and `HF_HUB_OFFLINE=1`, so they read `tmp/hf` and never the network.
- Prefer tests returning `anyhow::Result<()>` with `?` over chains of `unwrap()`.
- Vendored goldens live under `crates/<crate>/tests/data/` with their upstream license notice. The decider goldens in
  `crates/ardana-core/tests/data/decider/` are regenerated only by `cargo xtask export-decider`.
- Tests that read only tokenizer or config files from `tmp/hf` (no weights) are not `#[ignore]`d: they are fast and
  offline once `cargo xtask fetch` has run, and fail with a message naming that command when the file is missing.

### Lints and formatting
- `cargo clippy --workspace --all-targets -- -D warnings` must pass before every commit; this also fails on rustc
  warnings such as `dead_code`.
- Silence a lint locally with `#[expect(clippy::<lint>, reason = "...")]` on the smallest item (`expect` warns once
  the lint no longer fires, so stale suppressions surface); never crate-wide without a note in this file.
- `cargo fmt --all -- --check` must pass; keep rustfmt at its defaults, and add a `rustfmt.toml` only for stable options.

## CI
- `.github/workflows/ci.yml` runs on every pull request and every push to `main`, in parallel jobs: `fmt`
  (`cargo fmt --all --check`), `clippy` (`--workspace --all-targets --locked -- -D warnings`, then
  `cargo check -p ardana-api -p ardana-playground --target wasm32-unknown-unknown`), project checks (`check-deps`,
  `check-docs`, `dist generate --check`, `dist plan`), `test` on `ubuntu-24.04` and `macos-15`
  (`cargo xtask fetch --tests`, then `cargo test --workspace --locked`), a Windows build
  (`cargo build --workspace --exclude xtask`, as xtask is Unix only) and the playground build the release runs
  (`cargo install trunk --version 0.21.14 --locked`, `trunk build --release`).
- The `#[ignore]`d real-model tests and the `cargo xtask e2e` suites stay local: they need the GGUF weights and the
  Ollama store.
- Follow the common Rust CI practice (ruff, uv, bevy): `permissions: contents: read`, `CARGO_INCREMENTAL=0`, superseded
  pull-request runs cancelled, `actions/checkout` with `persist-credentials: false`, every action pinned to a commit
  SHA with its version in a comment, and `Swatinem/rust-cache` saving only from `main`
  (`save-if: ${{ github.ref == 'refs/heads/main' }}`) so pull requests reuse `main`'s cache without churning it.
- The tests' Hub files (`tmp/hf`, about 60 MB, no weights) are cached per OS under a key hashed from
  `xtask/fetch.toml`. They are the library models' files only, so CI uses no secrets and pull requests from forks
  run the same jobs.

## Releases
- dist 0.32.0 owns releases: `dist-workspace.toml` (`[dist]`) holds the config, `[profile.dist]` in the root
  `Cargo.toml` (inherits `release`, `lto = "thin"`) the build profile, and `.github/workflows/release.yml` is
  generated from them. Never edit `release.yml` by hand: change the config or `.github/build-setup.yml`, run
  `tmp/bin/dist generate`, and commit both; `tmp/bin/dist generate --check` fails on drift.
- Only `ardana` is released: its manifest sets `[package.metadata.dist] dist = true` over the workspace's
  `publish = false`, and takes `repository` (`https://github.com/ardana-ai/ardana`, needed for GitHub CI and the
  installers' download URLs) from `[workspace.package]`. `precise-builds = true` makes dist run
  `cargo build --package ardana` instead of building the whole workspace.
- Targets and runners: `aarch64-apple-darwin` on `macos-14`, `x86_64-apple-darwin` on `macos-15-intel`,
  `aarch64-unknown-linux-gnu` on `ubuntu-22.04-arm`, `x86_64-unknown-linux-gnu` on `ubuntu-22.04` (glibc 2.35 floor)
  and `x86_64-pc-windows-msvc` on `windows-2022`, all standard free runners; the images ship CMake and Clang, so the
  workflow installs no build tools. The x86_64 llama.cpp baseline and the static CRT come from `.cargo/config.toml`
  `[env]` (`llama-cpp.md`, "Build"); dist adds `+crt-static` on MSVC through `RUSTFLAGS`.
- Every build job runs `.github/build-setup.yml` first (the playground, `leptos.md`), so the released binary embeds
  the real playground.
- Artifacts: a `.tar.xz` per Unix target, a `.zip` for Windows, `ardana-installer.sh` and `ardana-installer.ps1`
  (`install-path = "~/.local/bin"`, `%USERPROFILE%\.local\bin` on Windows), checksums and the source tarball.
- Releases start only by hand (`dispatch-releases = true`, `pr-run-mode = "skip"`; pushes, tags and pull requests run
  nothing). Actions → "Cut release" (`.github/workflows/cut-release.yml`) takes `patch`, `minor` or `major`, bumps
  `[workspace.package] version` and the workspace entries of `Cargo.lock` (`cargo update --workspace`), commits
  `Release vX.Y.Z` and the tag `vX.Y.Z` to `main`, and runs `gh workflow run release.yml --ref vX.Y.Z -f tag=vX.Y.Z`
  (a tag pushed with `GITHUB_TOKEN` starts no workflow; a dispatch does). dist checks that the tag equals the
  version, builds every target and publishes the GitHub Release at the tagged commit. The version must be plain
  `MAJOR.MINOR.PATCH`. `main` must accept pushes from `github-actions[bot]`.
- Run "Release" (`release.yml`) alone with its default tag `dry-run` to build every target without publishing. To
  retry a release whose build failed after the bump, run "Release" with the existing tag instead of cutting again.
- Leave no artifacts behind: they only carry files between the jobs of one run, and the GitHub Release keeps the
  published files. Every `actions/upload-artifact` step in a workflow we write sets `retention-days: 1`, as uv, ruff,
  zed and typst do. `release.yml` cannot (dist's template sets no retention, and maintaining it by hand as uv and
  ruff do with `allow-dirty = ["ci"]` gives up `dist generate`), so `.github/workflows/cleanup-artifacts.yml`
  (`workflow_run` of "Release", `completed`) deletes every artifact of each finished Release run: published, failed,
  cancelled or dry-run. dist's custom jobs cannot do it: they run only after the jobs before them succeed.
- Check a release locally without pushing: `tmp/bin/dist plan`, then
  `tmp/bin/dist build --artifacts=local --target aarch64-apple-darwin` (binary in `target/aarch64-apple-darwin/dist/`,
  archive in `target/distrib/`) and `tmp/bin/dist build --artifacts=global` (installers).

## xtask
- `xtask` is a workspace member binary with subcommands `env [--claude]`, `fetch [--check|--tests]`, `build`, `check-deps`,
  `check-docs`, `export-decider`, `e2e <suite>` (`smoke`, `rust`, `jevcompat`, `sdk`, `jevbench`, `playground`,
  `design`); keep its
  dependency set small so `cargo xtask` compiles quickly: downloads and HTTP probes go through `curl` and `git` run by
  `Sandbox::command`, not HTTP crates. Its one codec is `lz-str` (the `design` suite's Jev share links).
- The API suites (`jevcompat`, `sdk`, `jevbench`) build `ardana` with `--release --locked` (llama.cpp in a debug build
  is too slow for 231 JevBench items) and run it through `xtask/src/serve.rs#Server`, which kills the server on drop.
- Run every external tool through `Sandbox::command` (sandbox env, `PATH` prefixed with `tmp/bin`), and wrap `fetch`,
  `build` and every `e2e` suite in `Sandbox::guarded`.
- Install cargo tools only through `Sandbox::cargo_install` (`CARGO_HOME=tmp/cargo`, root `tmp`); never
  `cargo install` into `~/.cargo/bin`.
- Build the playground with `cargo xtask build`, never from a `build.rs`: a build script that runs trunk runs a nested
  cargo that blocks on the outer build's target-dir lock (rust-lang/cargo#8938). `ardana-server`'s build script may
  only read `crates/ardana-playground/dist` (and warn when it is missing), never build it.

## Sources
- https://doc.rust-lang.org/cargo/reference/workspaces.html — virtual manifests, `workspace.dependencies`, `workspace.package`, `workspace.lints`, shared lockfile and target dir
- https://doc.rust-lang.org/cargo/reference/resolver.html — resolver 3 default for edition 2024, explicit resolver in virtual workspaces
- https://doc.rust-lang.org/cargo/reference/config.html — `[env]` with `relative`/`force`, `[alias]`
- https://doc.rust-lang.org/cargo/reference/environment-variables.html — `CARGO_BIN_EXE_<name>`, `CARGO_MANIFEST_DIR`, `CARGO_TARGET_TMPDIR`
- https://doc.rust-lang.org/cargo/faq.html — committing `Cargo.lock` for binaries, `--locked`
- https://doc.rust-lang.org/cargo/commands/cargo-test.html — `--workspace`, arguments after `--`, `--locked`
- https://doc.rust-lang.org/edition-guide/rust-2024/index.html — edition 2024, stabilized in Rust 1.85
- https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html — `set_var`/`remove_var` unsafe
- https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html — `?` and `From` conversions
- https://doc.rust-lang.org/book/ch11-02-running-tests.html — `#[ignore]`, `--include-ignored`, parallel tests
- https://doc.rust-lang.org/book/ch11-03-test-organization.html — unit vs integration tests, `tests/common/mod.rs`
- https://doc.rust-lang.org/reference/attributes/testing.html — `#[ignore = "reason"]`, tests returning `Result`
- https://doc.rust-lang.org/reference/attributes/diagnostics.html — lint `reason` parameter, `#[expect]`
- https://doc.rust-lang.org/clippy/usage.html — `cargo clippy`, `-D warnings`, lint levels
- https://rust-lang.github.io/api-guidelines/interoperability.html — C-GOOD-ERR, C-COMMON-TRAITS
- https://docs.rs/thiserror/latest/thiserror/ — derive for library error types; points to anyhow for applications
- https://docs.rs/anyhow/latest/anyhow/ — `Context`, `bail!`, `ensure!`
- https://docs.rs/clap/4/clap/_derive/index.html — `Parser`, `Subcommand`, `Args` derive for the `ardana` CLI
- https://docs.rs/indexmap/2/indexmap/ — insertion-ordered maps with `serde` support
- https://github.com/rust-lang/rustfmt — `cargo fmt --all -- --check`, `rustfmt.toml`, `style_edition`
- https://github.com/rust-lang/cargo/issues/8938 — nested cargo from a build script deadlocks on the target-dir lock
- https://github.com/matklad/cargo-xtask — xtask alias, workspace member layout, keep xtask fast to compile
- https://docs.rs/cargo-dist/0.32.0 — dist config (`dist-workspace.toml`), `precise-builds`, `github-build-setup`, `install-path`, `pr-run-mode`
