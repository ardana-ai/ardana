# Ardana

Ardana is a local-first tool that pulls and runs open System 1 decision models (decider-style one-pass readouts) the
way Ollama does, from one `ardana` Rust binary that serves them over an HTTP API (compatible with Jev and TypeSafe
clients) and bundles a Rust/WASM playground. llama.cpp is the first runtime behind the `Runtime` abstraction (GGUF
files) and Candle the second (safetensors causal LMs, and the THX-01 decision model); Qwen3.5 is the first model family.
`ardana finetune` turns any model it runs into a calibrated model of the user's own (TRL in a venv it builds, llama.cpp's
converter and the linked quantizer, or a safetensors checkpoint Candle runs, temperatures fitted and a test report
against the base). Planning documents live in
gitignored `*-prd/` folders; when one is present locally, its Decisions and Contracts are binding (as its Amendments
change them) and its Requirements are the definition of done.

## Workspace

| Crate | Role | May depend on (workspace crates) |
|-------|------|----------------------------------|
| `crates/ardana` | binary, wires runtimes into registry and server | any |
| `crates/ardana-api` | Jev wire types and error bodies, compiles for `wasm32-unknown-unknown` | none |
| `crates/ardana-core` | prompts, tokenizer, readout, runtime traits (causal and decision models), THX-01's sequences, safetensors snapshot files, fine-tuning data and rows | `ardana-api` |
| `crates/ardana-llama` | llama.cpp runtime and GGUF quantization | `ardana-core` |
| `crates/ardana-candle` | Candle runtime for safetensors causal LMs and THX-01, by `config.json#architectures` | `ardana-core` |
| `crates/ardana-registry` | refs, the model library, `models.toml`, HF and Ollama resolution, a fine-tune's base | `ardana-core` |
| `crates/ardana-server` | axum server, model lifecycle, embedded playground | anything but the runtimes (`ardana-llama`, `ardana-candle`) |
| `crates/ardana-playground` | Leptos CSR playground built by trunk, running browser models in the tab (onnxruntime-web) | `ardana-api` |
| `crates/ardana-engine` | the in-tab engine's tokenizer, planner and readout (ardana-core), a second wasm module | `ardana-api`, `ardana-core` |
| `xtask` | task runner | any |

`cargo xtask check-deps` enforces the direction.

## Commands

- `cargo xtask env` prints the sandbox environment as shell exports; `cargo xtask env --claude` writes it (all but
  `HOME`) into `.claude/settings.local.json` `env`.
- `cargo xtask fetch` installs the `xtask/fetch.toml` entries into `tmp/` (tools, research clones in `tmp/src`
  (decider, jevcompat, JevBench, llama.cpp b11074), the jevcompat uv tool, the `tmp/py/sdk` and `tmp/py/jevbench`
  venvs, and of every `[[hf]]` repository the files that are no weights, in `tmp/hf/hub`);
  `cargo xtask fetch --check` verifies them. It also runs `npm ci` in `e2e/playground` (`@playwright/test`,
  `lz-string`, `@typesafe-ai/sdk`) and copies the impeccable binary to `tmp/impeccable/bin/0.1.5/impeccable`. Run it
  before `cargo test`: plain tests read the library models' (decider:2b, Qwen3.5, SmolLM3, Gemma 4, Qwen3.8,
  Llama 3.2) tokenizers, configs and chat templates (Qwen3.5-0.8B's `config.json`, whose architecture `ardana finetune
  --format safetensors` refuses, among them), and Qwen3-1.7B's (with its GGUF repository's card, `README.md`, whose
  `base_model` `ardana finetune` reads), from `tmp/hf`. `cargo xtask fetch --tests`
  installs only those files (every `[[hf]]` file but the GGUF, ONNX and safetensors weights, no token), which is what
  CI runs. No fetch installs weights: `[[hf]]` pins them, and the `cargo xtask e2e` suite that reads them downloads
  them into `tmp/hf` and removes them when it ends (`xtask/src/hub.rs`).
- CI (`.github/workflows/ci.yml`) checks every pull request and push to `main`: `cargo fmt --check`, clippy with
  `-D warnings` and a wasm check of `ardana-api`/`ardana-playground`/`ardana-engine`, `check-deps`, `check-docs`,
  `dist generate --check` and `dist plan`, `cargo test --workspace` on Linux and macOS (after `fetch --tests`; no
  secrets), a Windows build and the release's trunk playground build. The real-model
  suites stay local. `rust-toolchain.toml` pins Rust 1.97.1 for local, CI and release builds.
- `cargo xtask build` builds the playground `dist/` with the fetched trunk, offline (the page's wasm, and the engine
  module and `engine.js` in `dist/engine/`), then the release `ardana`, which embeds it (memory-serve, `force-embed`)
  and serves it at `/`. Building `ardana-server` without a dist (or with `ARDANA_PLAYGROUND_DIST` at a directory
  without `index.html`) warns and embeds a placeholder page.
- `cargo xtask build-playground --public-url <path> [--hub <url>] [--library <url>]` builds the standalone
  playground, which no server serves, into `tmp/playground/dist` and prints that path (`crates/ardana-playground/dist`
  stays as it is): trunk with the playground's `standalone` feature and `--public-url`, and no library baked (Q13; the
  build reads no snapshot and no hub cache). The page GETs `--library` (default `/models.json`, the C1 document) on
  load and whenever its tab comes back, lists it as a server that pulled nothing lists the library (one row per size
  its version reads, Q13, under its canonical name `<family>:<size>` in the document's order) with the
  `browser_default` row first offered, and says the library is unavailable, keeping its last list, when a GET fails
  or the document is another `schema`.
  Its "In browser" rows download `<hub>/<org>/<repo>/resolve/<browser.commit>/<file>` (default
  `https://huggingface.co`) and run with `browser.profile`; every other model shows ardana.ai's install line and
  `ardana run`. `npm run build-playground` in `../ardana-landing` runs it with `--public-url /playground/` and commits
  the result in its `static/playground/`, served at ardana.ai/playground/ beside ardana.ai/models.json.
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
- `cargo xtask export-thx01` regenerates the THX-01 goldens in `crates/ardana-core/tests/data/thx01/`
  (`{ticket,sentiment,score}.json`, the `thx01` 1.0.0 package's `system_one` answers to the three request fixtures on
  the CPU, with its license) in the uv venv `tmp/py/thx01-export` (CPU torch from PyTorch's index); it downloads the
  checkpoint's six files at the pinned commit into `tmp/thx01-export`, which it removes when done. Never hand-edit a
  golden.
- `cargo xtask onnx convert <name>` (a name meaning a size's default quant, `decider:0.8b`; another quant is refused)
  builds a library size's browser variant from the checkpoint its size in the library document,
  `../ardana-landing/src/lib/data/models.json` (the landing checkout beside this one; both steps stop naming it when it
  is absent), names as `source`: the int4 or int8 ONNX (onnxruntime-genai 0.17.1 model builder in
  the `tmp/py/onnx` venv it creates) and, when the size's `gguf.repo` is an ardana-ai repository (decider:0.8b), the
  GGUF its quant's `file` names (llama.cpp's `convert_hf_to_gguf.py` from `tmp/src/llama.cpp`). It writes the
  size's `browser.repo` (and `gguf.repo`) into `tmp/hf/hub` as local snapshots, records their commits, bytes and the
  browser profile in the size's entry of the document and copies it to the library snapshot (the two stay
  byte-identical, in the landing's Prettier formatting), and reads them back offline with the release `ardana pull` on
  that document; a size without a `browser` table is refused.
  `cargo xtask onnx publish <name> [--dry-run]` prints the model cards, the `hf repos create` and `hf upload`
  commands, the `[[hf]]` entries and the size's document entry, then uploads the snapshots to huggingface.co/ardana-ai with
  the user's token (`HF_TOKEN`, else the real `~/.cache/huggingface/token`) and pins them in `xtask/fetch.toml`, the
  document and the snapshot; a failed upload exits non-zero and changes nothing. Both run by hand only, never in CI
  (`docs/guidelines/onnx.md`).
- `cargo xtask e2e <suite>` runs an end-to-end suite, which needs no model on the machine beforehand: each downloads
  the weights it reads into `tmp/hf` at their `[[hf]]` pins and removes every path it added under `tmp/hf/hub` when
  it ends, also when it fails. The suites: `smoke`, `rust` (`cargo test --workspace` once, then each
  `xtask/src/e2e.rs#RUST_FIXTURES` row in turn: `decider:2b`, `qwen3.5:0.8b`, `smollm3:3b`, `gemma-4:e2b`, `library`
  (those four in one registry, and the `ollama:` reference on an Ollama store the test builds around qwen3.5:0.8b's
  GGUF, no Ollama install), `qwen3:1.7b` (the Candle checkpoint and its GGUF), `decider:0.8b` (the browser variant) and `update` (the GitHub
  Release, no weights); a row downloads its weights, runs the `#[ignore]`d tests it lists and removes the weights
  before the next, a failed row does not stop the others, and the suite fails before any download when an
  `#[ignore]`d test of the workspace is in no row or a row lists a test that does not exist), or one of the API
  suites against a release `ardana serve` on decider:2b: `jevcompat` (jevcompat 0.1.0, open and with a key, every MUST must pass), `sdk` (typesafe-sdk 0.7.2
  parses `ticket.json`) and `jevbench` (231 public items, zero failed requests; output in `tmp/evals/jevbench/`); the browser suites
  `playground` (Playwright on the installed Chrome against the built binary copied alone into `tmp/`, plus the
  placeholder build; the W6 cases, then the builder, state modes, the 17 docs.typesafe.ai share links in
  `e2e/playground/fixtures/jev-share-links.json`, the share round trip, the presets, the executed snippets (the
  ardana commands of an "In browser" row, `ardana pull` then `ardana run`, run with the binary under test),
  `run_command` (every row the server has not pulled, and every name no row carries, never split, shows `ardana pull
  <name>` and `ardana run <name> --request -` as written and no install line, and Run sends nothing, autorun
  included), `pull_while_open` against
  one more empty server (the shown `ardana pull qwen3.5:0.8b` run with the binary under test, the tab's return makes
  the row a server row, and Run answers from the server), `first_run`, `browser_stop` and `browser_recover` against
  one more server on an empty registry
  (the picker opens on decider:0.8b's "In browser" row and the first Run answers in the tab; an autorun link waits for
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
  (decider:0.8b's "In browser" row runs the ticket preset in the tab: no pull from a server that holds the files, at
  1 s and 600 ms of latency, the download's bytes as one stage, the stages said once to screen readers, the server's
  top answers on WebGPU and WASM, a server row picked mid-row releasing the session once that row is over, a reload
  that downloads under 1 MB, the server's 422 body; on a page that is no secure context, on WASM, keeping nothing; and
  through a proxy that drops the weights' connection, then after Stop, then after a reload, each next run resuming with
  `Range: bytes=<had>-` and a 206, the weights crossing the wire about once), `browser_pull` against one more server
  over an empty Hub cache (the first run in the tab says the server pulls, then that the server could not provide the
  files); `@design` and `@standalone` tests left out; screenshots in `tmp/screens/<case>/`, reports in
  `tmp/playwright/`), `standalone` (`cargo xtask e2e standalone`: `cargo xtask build-playground --public-url
  /playground/` with `--hub` at a stand-in of Hugging Face serving `tmp/hf` offline on another origin (the build
  writes no `tmp/playground/library.json` and its dist names no repository of the snapshot), the dist served as
  static files under `/playground/` with the landing's isolation headers and a copy of the snapshot at `/models.json`
  (an `ETag` and `max-age=0, must-revalidate`, as Cloudflare serves the landing's; the cases rewrite and take away the
  copy), both hosts from `e2e/playground/standalone-hosts.mjs`; the `@standalone` cases at both viewports:
  `standalone_first_run` (one GET of `/models.json`, every size listed in its order under its canonical name,
  decider:0.8b's "In browser" row,
  nothing under `/v1/`, fonts and onnxruntime-web from `/playground/`, no failed request), `standalone_browser_run`
  (the three files from the stand-in at the document's commit, the ticket answered as the release `ardana run`
  answers it, under 1 MB after a reload), `standalone_run_command` (a model without a browser variant and a share
  link's unknown model: the install line and `ardana run`, Run sends nothing, and "Run decider:0.8b in this tab
  instead" picks that row), `standalone_share` (`<origin>/playground/#share/` round trip), `standalone_library_update`
  (a size added to a family of the served document is in the picker, in the document's order, when the tab comes
  back, with no reload) and
  `standalone_library_unavailable` (the document gone before the load, and again while the page is open: the page
  says the library is unavailable, keeps its list, pick and editors, and lists again on the tab's next return; a
  family and a size for a later ardana and a family and a size that do not read are left out, another `schema` is
  unavailable))
  and `design`
  (/impeccable context, then `impeccable detect` of the empty, loaded, results, 422 and `run-command` states at
  1280x800 and 390x844, and of the in-tab `browser-download` and `browser-results` states the `@design` Playwright
  test freezes from a real run into `tmp/evals/design/<state>-<viewport>.html`, and of the standalone build's
  `library-unavailable` state (`/models.json` a 404 from `e2e/playground/standalone-hosts.mjs`, the page at
  `/playground/`), reports in `tmp/evals/design/`, then the finish: a `.impeccable/critique/` record,
  `docs/design/audit.md` with `P0: 0 · P1: 0`, clean scans, the hook enabled); and the Candle suite `candle`
  (`cargo xtask e2e candle`: each `xtask/src/e2e.rs#CANDLE_FIXTURES` row in turn, Qwen/Qwen2.5-1.5B-Instruct,
  HuggingFaceTB/SmolLM3-3B, unsloth/Llama-3.2-1B-Instruct, openbmb/MiniCPM5-1B, microsoft/Phi-4-mini-instruct,
  ibm-granite/granite-4.2-3b, LiquidAI/LFM2.5-1.2B-Instruct, unsloth/gemma-3-1b-it, unsloth/gemma-3-4b-it (a
  multimodal checkpoint, its text tower alone), allenai/Olmo-3-7B-Instruct, then the decision model doofz/THX-01,
  with the release binary on a home of the row's own.
  It pulls the safetensors checkpoint and the Q8_0 GGUF of the same model (the checkpoint repository as
  `--tokenizer`, `--layout chat`) from the Hub into `tmp/hf`, about 5 GB, 9.5 GB, 3.8 GB, 3.3 GB, 11.8 GB, 11.2 GB,
  3.6 GB, 3.1 GB, 12.8 GB and 22.4 GB, and checks each against the commit the row pins and the checkpoint's runtime,
  `candle`;
  `ardana run
  --json` answers `tests/fixtures/requests/ticket.json`, `sentiment.json` and `score.json` through both, Candle's
  answers with the GGUF's choices and noul sides and every probability within 0.1; one `ardana serve` answers the
  three through Candle and the first again with identical probabilities; a `long_state` row (Qwen2.5, Phi-4-mini,
  LFM2.5, both Gemma 3, Olmo 3) also answers the ticket's state repeated past 4,500 tokens through both. The `thx-01`
  row (no GGUF, 644 MB) pulls `hf.co/doofz/THX-01` as it is, checks the pin, runtime `candle`, the six files in its
  snapshot, `tokenizer/tokenizer.json` as the tokenizer and `ardana show`'s kind `decision`, answers the three
  fixtures with the `thx01` package's goldens (`crates/ardana-core/tests/data/thx01/`: the golden's choices, every
  probability within 0.01, the golden's `input_tokens`), is refused on the ticket's state repeated 300 times (naming
  the 1,024-token window) and on a 25-option choice (naming the 24-option limit), prints the model, tokens and timings
  under `--verbose` and is listed by its `model.safetensors`; then one `ardana serve` on its home answers the three
  fixtures at `/v1/systemone` with the goldens too (the same choices, every probability within 0.01, the golden's
  `input_tokens`) and the first again with identical probabilities, and `ardana ps` lists the model it loaded. Then,
  also when a check failed,
  the row's registry entries and every file it added under `tmp/hf/hub` are removed, the files that were there
  before staying, and after the last row the suite runs `cargo clean`: no `target/` is left, and the next cargo
  command builds from nothing. A failed row does not stop the others; the suite fails naming it); and the fine-tune
  suite `finetune` (`cargo xtask e2e finetune`: each `xtask/src/e2e.rs#FINETUNE_FIXTURES` row in turn with the
  release binary, online, on one home `tmp/e2e/finetune/home` that keeps the training environment from row to row and
  is removed when the suite ends, also on failure; the suite writes the first 1,000 lines of decider@23579f7's
  `teacher_data/commands.jsonl` as `commands.jsonl` and their two shared questions, `risk` (safe, caution,
  destructive) and the noul `outside`, as `commands.questions.json`, and the first 200 lines of its
  `teacher_data/custom_questions.jsonl`, each with its own noul, choice and score questions, as `custom.jsonl` in
  `tmp/e2e/finetune/`, then starts one `ardana serve` on the home that runs until the suite ends. A row that answers
  fine-tunes its model three times: stopped by SIGINT past the trainer's first step (row `qwen3:0.6b`'s is the run
  that installs uv 0.13.1, CPython 3.12, the Q29 pins and the converter, which the row then checks), then with
  `trl/scripts/sft.py` moved out of the venv so the trainer exits 1, neither leaving an entry, `models/<name>/` or a
  working directory, then to the end: `ardana list` lists it, its directory holds `<name>-<QUANT>.gguf` (that file
  type), the base's tokenizer and chat template files, `decider_config.json` (the base's layout, the temperatures the
  run says it fitted, by Q30, which `ardana show --json` prints) and `finetune.json` (the split's lines, the report
  its stdout prints: the base, pulled from its default quant, and the model per answer type and overall), the
  model's overall test accuracy is as the row's `Gain` says against the base's, `ardana run <name> --request -`
  answers the first data line's questions, the running server lists the model in `/v1/models` and answers the same
  request at `/v1/systemone` with the answers `ardana run --json` gave, and `ardana rm` removes the entry and the
  directory; every row after the first that answers finds the environment's files unchanged and downloads none of
  it. Rows: `qwen3:0.6b` (`cmd-risk` on the commands data, chat, Q8_0, above the base; 81 to 90 minutes of training
  on 8 CPU cores), `thx-01` (`hf.co/doofz/THX-01` exits 1 named a decision model, no weight file reaching `tmp/hf`),
  `decider:0.8b` (`custom-decider` on the custom data, plain with isolated score levels, Q8_0, `--epochs 1`, at least
  the base's accuracy minus 0.05) and `qwen3.5:0.8b` (`cmd-risk-qwen3.5` on the commands data, the multimodal
  `Qwen3_5ForConditionalGeneration` trained as its text tower, chat, Q4_0, above the base), `hf.co-gguf`
  (`cmd-risk-gguf` from `hf.co/Qwen/Qwen3-0.6B-GGUF:Q8_0` on `commands.csv`, the same lines as CSV: trained from
  `Qwen/Qwen3-0.6B`, the `base_model` of the repository's card, recorded with its commit, the base answering as
  `ardana pull` resolves the reference, not recorded; then `cmd-risk-gguf-local` from that GGUF's file in `tmp/hf`
  with `--checkpoint <the checkpoint's snapshot directory>` and `--epochs 1`, recorded as `{"path"}`) and
  `spark-x2.5:1.7b` (`cmd-risk-spark`, `--trust-remote-code --epochs 1`: first run without the flag, which exits 1
  naming it with no weight file reaching `tmp/hf`, as `XHToken/Spark-X2.5-1.7B`'s `config.json` has `auto_map`; then
  trained, converted and answering, Q8_0); both rows check that the model answers, not its accuracy; and `safetensors`
  (`cmd-risk-st` from `hf.co/Qwen/Qwen3-0.6B` on the commands data, `--format safetensors`: its directory holds the
  checkpoint's `config.json` in float32 and the merged `model.safetensors`, every tensor F32, besides the tokenizer
  files, `decider_config.json` and `finetune.json` (`"format": "safetensors"`, `quant` null), `ardana show` prints
  runtime `candle`, the run says it calibrated through `candle`, the base answering as `ardana pull` resolves the
  checkpoint, through Candle too, above the base). Every row's run says the runtime it calibrated through, llama.cpp
  for a GGUF. Each row removes every entry it added (its model, the base it pulled), its model directory and every
  path it added under `tmp/hf/hub` when its checks end, also when one failed; a failed row does not stop the others).
- `ardana pull <name|ref> [--tokenizer hf.co/<org>/<repo>|<path>] [--name N] [--layout plain|chat]` records a model
  in `$ARDANA_HOME/models.toml` (default `~/.ardana`; `tmp/ardana` under cargo), printing download progress on
  stderr. The library is a list of families with their sizes inside (`decider`: `0.8b`, `2b`, `4b`), each size one
  checkpoint with its GGUF quants (`gguf.quants`, the default first) and at most one browser variant. A library name
  is `<family>[:<tag>]`: a family's tags are `latest` (the size its `latest` names, also the family alone), `<size>`
  for a size's default quant and `<size>-<quant>` for every quant, listed and never parsed, family and tag matched
  ASCII case-insensitively; a tag the family lacks fails listing its tags (`gemma-4 has no tag "9b"; its tags are
  e2b, e2b-q8_0, …`). A pull downloads the file the quant names at `gguf.commit` and records it under its canonical
  name, `<family>:<size>` or `<family>:<size>-<quant>` (`decider`, `decider:latest` and `decider:2b-q4_k_m` all record
  `decider:2b`), its source `hf.co/<org>/<repo>:<file>`; a size without its own `decider_config.json` gets the stock
  profile named canonically. The library is one published document, never compiled in: `ARDANA_LIBRARY` names its
  source (unset: `https://ardana.ai/models.json`, with each family's manifest at `models/<family>.json` beside it; an
  `http(s)://…/models.json` URL: that index; a file path: that one document, no network; `off`: no library), and the
  family `decider` is the one library name the binary keeps. `pull` and `run` of a model not pulled GET its family's
  manifest every time (`User-Agent: ardana/<version>` and nothing else) and keep the bytes at
  `$ARDANA_HOME/library/models/<family>.json`, which stands in when the GET fails; `list`, `show`, `rm`, `ps` and help
  read that cache only, and help and error texts name no library model but `decider`, write any other as
  `<family>:<size>-<quant>` and point to `https://ardana.ai/models/`. A document with unknown fields reads, a size or
  a family that does not read, whose repository is no `hf.co/<org>/<repo>`, whose commit is no 40-hex one or whose
  `min_version` is above the binary's is left out (a family with no size left too), and another `schema` is refused
  with an error saying to update `ardana`. A size's `kind`, `summary`, `license` and `layout` are its own, else its
  family's. A size's browser variant is its `browser` table (decider:0.8b, decider:2b and qwen3.5:0.8b today,
  `browser_default` decider:0.8b); refs are
  `hf.co/<org>/<repo>[:<quant>]` (default Q4_K_M), `hf.co/<org>/<repo>:<file>.gguf`, `ollama:[<ns>/]<name>[:<tag>]`
  (read in place from `$OLLAMA_MODELS`) and local GGUF paths. `hf.co/<org>/<repo>` of a repository with no GGUF and a
  `config.json` pulls that safetensors checkpoint at `main` (`config.json`, the shards its index names or its
  `model.safetensors`, the root tokenizer files), recorded with its snapshot directory as the weights and runtime
  `candle` when `config.json#architectures` names one Candle implements (`Qwen3ForCausalLM`, `Qwen2ForCausalLM`,
  `SmolLM3ForCausalLM`, `LlamaForCausalLM`, `Phi3ForCausalLM`, `GraniteForCausalLM`, `Lfm2ForCausalLM`,
  `Gemma3ForCausalLM`, `Gemma3ForConditionalGeneration` as its text tower alone, read from `text_config` and the
  `language_model.` tensors, and `Olmo3ForCausalLM` with its sliding layers and YaRN rope scaling), and refused
  naming the architectures before any weight file when none does; `list` sizes it by its safetensors files. A
  checkpoint with an `rl_agent_config.json` is a THX-01 decision model (`THXDecisionModel`, `hf.co/doofz/THX-01`,
  never in the library): the pull takes its `config.json`, `rl_agent_config.json`, `encoder/config.json`,
  `tokenizer/tokenizer.json`, `tokenizer/tokenizer_config.json` and `model.safetensors`, records
  `tokenizer/tokenizer.json` as the tokenizer and the profile from `rl_agent_config.json` (`decision`: the 1,024-token
  window, the 24-option limit and the head budget; the clamped base temperatures), and `ardana show` prints the kind
  `decision`. Such a model answers one sequence per question in the `thx01` package's format, every probability
  within 0.01 of the package's (its goldens, `cargo xtask export-thx01`); a question whose sequence would exceed the
  window is refused as too large naming the window, never truncated, and a choice question above 24 options as
  invalid naming the limit; its `number` and `excerpt` questions, `action` output and tournament are not implemented.
  Downloads
  go to the
  standard HF cache (`$HF_HOME/hub`); `HF_HUB_OFFLINE=1` resolves `hf.co/` refs from the hub cache only. `ardana list`
  (`ls`: name, size, pull date, source), `ardana show <name> [--json]` (Model, Calibration and Files sections; a
  library model not pulled yet says how to get it) and `ardana rm <name>...` (the entries, named as `list` prints
  them, every name checked first; a model `ardana finetune` made also loses its `$ARDANA_HOME/models/<name>/`, any
  other's files stay) manage
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
  [--keep-alive 5m] [--library-refresh 1h] [--max-loaded-models 1] [--max-queued-rows 4096]`, each flag also read
  from `ARDANA_<FLAG>` (`ARDANA_PORT`, `ARDANA_API_KEY`, `ARDANA_LIBRARY_REFRESH`, ...), serves the registry's models
  and the library's. The library (`ARDANA_LIBRARY`, default `https://ardana.ai/models.json`) is read once on start and
  once every `--library-refresh`, in the background, so binding never waits on the network: under a URL source one
  index GET with `If-None-Match` on the `ETag` kept beside the cached index (`$ARDANA_HOME/library/models.json`,
  `models.json.etag`), a 200 replacing what is served, a 304 or a failed GET keeping it (the cached index at start,
  the pulled models alone when there is none; the failure logged once, naming the URL), so a model added to the
  published document is listed and pulled without a restart; the first request after start waits for the first read.
  A name the index does not resolve (a family it lacks, or a tag its family lacks) is asked of the library by its
  family's manifest, whose answer (a 404 as no model) stands until the next refresh; a tag no one lists is a 404
  naming the family's tags, and nothing is pulled. A file source is read again the same way; `ARDANA_LIBRARY=off` sends nothing and serves the pulled models
  alone, a request without a model resolving to `decider`. `ardana serve --help` states the URL, the interval and `off`. The routes:
  `POST /v1/systemone`, `GET /v1/models` (pulled models, then one row per library size not pulled, under its
  canonical name, with `x_pulled: false` and `x_size`, its default quant's bytes; `x_default` marks the default,
  `x_browser` the bytes of a browser variant, `x_browser_pulled` a browser variant the hub cache holds whole,
  `x_browser_default` the library's `browser_default`),
  `GET /health`, and
  `GET /v1/browser/<name>/{model.onnx,model.onnx.data,tokenizer.json,profile}` (a library size's browser variant, the
  canonical name looked up whole, from the hub cache, pulled once on its first request, cache first; the three files
  also by byte range,
  `Accept-Ranges: bytes`, a 206 for one range, `If-Range` on the `ETag`, a 416 past the end, so a tab resumes a
  download that stopped; the playground's "In browser" rows run it in the tab with the engine module
  (`crates/ardana-engine`, at `/engine/`, imported on the pick) and onnxruntime-web, vendored in
  `crates/ardana-playground/ort/`). `/engine/` goes out with `Cache-Control: no-cache` (its names never change),
  every other asset with memory-serve's week. Every response carries COOP
  `same-origin`, COEP `require-corp` and CORP `same-origin`. A request naming a library model that is not pulled pulls it first
  (concurrent requests share the pull); a request without a model, or with the compatibility alias `jev-latest` or any
  `jev-*` name, uses the default model: `--default-model` (any spelling, as the canonical name it means), else the
  first pulled model, else the document's `default` (`decider:2b`); every spelling of a model reaches its one
  entry, and `/health` and `ardana ps` print the canonical name. The
  playground speaks only real model names, opens on the default model when it is pulled and else on decider:0.8b's
  "In browser" row, and never makes the server pull: it knows the names `/v1/models` lists and nothing else (a name
  equal to a row's, ASCII case ignored, is that row, and the page never splits a name), runs only a pulled row on the
  server, and for every other name (a row not pulled, a family, another quant) shows `ardana pull <name>` as written,
  to run where the server runs, and `ardana run <name> --request -` with the exact
  request instead, as the snippets do for an "In browser" row (the pull only while the server lacks the model); the
  page lists the models again after each run and whenever its tab comes back, so a model pulled meanwhile runs on the
  server.
  `models.toml` is reread when it changes, so a model `ardana pull` adds while `serve` runs is servable at once;
  models load on their first request and unload after `--keep-alive` idle.
- `ardana finetune <base> <data.jsonl|data.csv> --name N [--questions F] [--epochs 2] [--format gguf|safetensors]
  [--quant Q] [--checkpoint hf.co/<org>/<repo>|<dir>] [--trust-remote-code]` (Linux and macOS; elsewhere exit 1)
  trains a LoRA of the base's transformers checkpoint into a System One model (Q7). The base is any model `ardana`
  runs, looked up as a library size, then a registry entry, then a reference: a library size trains from its pinned
  checkpoint (a decider size's
  `source`, an instruct size's `tokenizer` repository), an `hf.co/<org>/<repo>` safetensors checkpoint from itself
  (at `main`), an `hf.co/` GGUF reference and an entry pulled from one from the `base_model` its repository's card
  (`README.md` front matter: a scalar, a flow or a block list) names when it names exactly one repository;
  `--checkpoint` names it for any base (a Hub repository at `main`, or a directory with `config.json` and its
  safetensors, recorded `{"path"}`), and an `ollama:` reference, a local GGUF file, decider:4b or a card naming none or
  several are refused naming `--checkpoint`. A checkpoint whose `config.json` has `auto_map` is refused naming
  `--trust-remote-code` before any weight downloads; with the flag the trainer and `merge.py` get
  `trust_remote_code`. A base other than a library size reads its tokenizer and profile as `ardana pull` derives them
  (an entry's as recorded; a GGUF without a `tokenizer.json` beside it the checkpoint's), its license from its card,
  else the checkpoint's, and its quant from the reference or the GGUF's file name, else `q4_k_m`. A data line is
  `{"state", "questions"?, "answers": {<id>: <label>}}`, a line without questions asking `--questions` (a map, or a
  `/v1/systemone` body), a label the option's name, `true`/`false`, a level's index or legend key, or `null`; a
  `.csv` file holds the same lines as a `state` column and a column per question id of `--questions` (required), each
  cell a label, an empty cell unlabelled, a missing `state`, an unknown or repeated column refused; the lines are
  split by line, seeded, 80/10/10 into train, calibration and test, under 100 labelled lines refused and an option
  under 50 training examples warned. The name (an entry's, a library family's or a library name's is refused, Q27),
  the base's checkpoint (its listing and `config.json`; a checkpoint with `rl_agent_config.json` refused as a decision
  model), its tokenizer and profile and every line (errors name `<file>:<line>` and the question) are checked before
  anything heavy. Then the first run builds
  `$ARDANA_HOME/finetune/` (uv 0.13.1 from its GitHub release and llama.cpp 26394b4's source archive for
  `convert_hf_to_gguf.py`, `conversion/` and `gguf-py/`, both sha256-checked against digests compiled in; uv's
  CPython 3.12 and a venv with the Q29 pins, torch from PyTorch's CPU wheels), which later runs reuse; the checkpoint
  downloads to the HF cache; TRL's `sft` script trains on the very rows `Decider::plan` builds (LoRA r16, alpha 32,
  all linear layers, lr 1e-4, the epochs, seed 0, the epoch with the lowest calibration loss kept, float32 on the
  CPU, offline); `merge.py` merges (`merge_and_unload`); the converter writes an f32 GGUF and the linked
  `llama_model_quantize` the `--quant`, default the base's own GGUF quant. `--format safetensors` writes no GGUF: the
  model is the merged checkpoint, its safetensors in float32 beside the checkpoint's own `config.json` (dtype float32;
  transformers 5's `save_pretrained` config, `rope_parameters` for `rope_theta`, does not read in Candle), registered
  with its directory as the weights and runtime `candle`. It takes exactly the causal architectures of
  `ardana_candle::ARCHITECTURES` (all but `THXDecisionModel`, so it grows with Candle): any other, read from the
  checkpoint's `config.json` (qwen3.5:0.8b's `Qwen3_5ForConditionalGeneration`), is refused before any weight
  downloads, naming it and `--format gguf`, and `--quant` with it is refused (no GGUF to quantize). A multimodal
  checkpoint (`*ForConditionalGeneration` with a `text_config`, Qwen3.5's) trains, merges and converts as its text
  tower alone.
  The base the report reads comes after the checkpoint: a library size pulled as `ardana run <base>` pulls it (its
  default quant and profile), an entry as recorded, a reference resolved as `ardana pull` resolves it, not recorded. The
  model's temperatures are then fitted on the calibration split through the model itself, the GGUF through llama.cpp
  and a checkpoint through Candle, the run saying `calibrated <name> through <runtime>` (Q11, Q30, decider's
  `fit_by_type`: `temperature` over every answer, a `temperature_by_type` entry for a type with 50 answers or more, by
  NLL over a log grid of 0.05 to 20 refined by golden section), and the base and the model answer the test split:
  stdout prints accuracy (noul: P(yes) ≥ 0.5), NLL, Brier and 15-bin ECE per answer type and overall for both.
  `$ARDANA_HOME/models/<name>/` then holds `<name>-<QUANT>.gguf` (or `config.json` and the safetensors), the base's
  tokenizer and chat template files, `decider_config.json` (the base's layout and score-level mode, the fitted
  temperatures, which `ardana show` prints) and `finetune.json`
  (`{base, checkpoint, format, quant, device, dtype, qlora, epochs, rows, license, report}`, `report` the same
  numbers), and it is registered as `<name>`, which `ardana run` and a running `ardana serve` answer. The model is
  built in `$ARDANA_HOME/finetune/work/<name>/`, removed whatever the end; SIGINT (listened for, children killed) and
  a failing step leave no entry and no `models/<name>/`. The venv compiles its bytecode at install and Python runs
  with `-B`, so later runs change none of the environment's files.
- `ardana update` installs the latest GitHub Release over an ardana the installers placed, through axoupdater: it
  reads dist's install receipt (`ardana-receipt.json` in `$XDG_CONFIG_HOME/ardana`, then `~/.config/ardana`;
  `%LOCALAPPDATA%\ardana` on Windows), looks ardana-ai/ardana's `releases/latest` up once and prints `ardana is up to
  date (<version>)`, or runs that release's `ardana-installer.sh` (`.ps1`) into the receipt's directory and prints
  `updated ardana from <old> to <new>`. A copy with no receipt for it (a source build, `target/release/ardana`) exits
  1 with the install line, and a failed lookup exits 1 with `the update check failed` within 30 s.
  `ardana run` and `ardana serve` (once, at start) make the same lookup at most once a day per `ARDANA_HOME`, on a
  thread of its own, so no answer, bind or model load waits on it: `$ARDANA_HOME/update-check.json` records the time
  when a lookup starts (`checked_at`, Unix seconds) and the version once it answers (`version`: the latest release
  when it is newer, else the binary's own), a failed lookup standing for the day like an answered one and keeping
  the version seen before (the file is dated under its own lock, so ardanas started together
  send one lookup). While that version is above the binary's, `run` prints `a newer
  ardana is available: <latest> (this is <version>); run `ardana update`` as its last stderr line, after its answers
  and without waiting for a lookup still in flight, and `serve` logs it once after `ardana serve: `, from the file
  alone within the day. The check adds nothing when its lookup fails, sends and writes nothing for a copy without a
  receipt (every cargo-built `ardana`, which still prints what a file an installed copy dated today says), and `ARDANA_NO_UPDATE_CHECK=1` turns it off (`ardana update` ignores that);
  `pull`, `list`, `show`, `ps` and `rm` never check. `crates/ardana/tests/update.rs` runs both against a loopback
  stand-in of GitHub's API (`ARDANA_INSTALLER_GHE_BASE_URL`, the receipt's directory in `AXOUPDATER_CONFIG_PATH`);
  the real download and install is its `#[ignore]`d network test, into a scratch directory.
- Running Ardana by hand: under `cargo run` (and `cargo test`), `.cargo/config.toml` puts `ARDANA_HOME` in
  `tmp/ardana`, `HF_HOME` in `tmp/hf` and `ARDANA_LIBRARY` at the library snapshot, so `cargo xtask build` then
  `cargo run --release -p ardana -- serve` serves the sandbox: its playground opens on decider:0.8b's "In browser" row while nothing is pulled, and an API request
  pulls decider:2b from the Hub into `tmp/hf`, where it stays until removed by hand (no xtask step keeps weights
  there).
  The release binary run directly (`target/release/ardana serve`) uses `~/.ardana`, the standard HF cache
  (`$HF_HOME`, else `~/.cache/huggingface`) and the published library at ardana.ai, like any installed tool; do not do
  that from an agent session.
- Before reporting work: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace`, `cargo test --workspace`, plus the item's Verify command.

## Sandbox

- Every download, cache, model, tool, venv, clone and temp file of builds, tests, evals and agent commands stays in
  the gitignored `tmp/` at the repo root (plus the gitignored `e2e/playground/node_modules`); `rm -rf tmp
  e2e/playground/node_modules` fully resets. Nothing is written elsewhere under the home directory, and no test
  or suite reads a model from it: no Ollama store, no Hugging Face cache. Model weights are in `tmp/hf` only while
  the suite that downloaded them runs.
- `.cargo/config.toml` `[env]` forces `HF_HOME`, `ARDANA_HOME`, `ARDANA_TMP` and `TMPDIR` into `tmp/` for every
  cargo-run process, including tests, and points `ARDANA_LIBRARY` at the library snapshot
  (`crates/ardana-registry/tests/data/models.json`, a byte-for-byte copy of the landing's `src/lib/data/models.json`),
  so no test reads the library from ardana.ai; `Sandbox::env` hands every tool the same.
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
  (Mapika/decider-2b-GGUF Q4_K_M, ggml-org/Qwen3.5-0.8B-GGUF, ggml-org/SmolLM3-3B-GGUF, ggml-org/gemma-4-E2B-it-GGUF, and
  for Candle Qwen/Qwen3-1.7B against Qwen/Qwen3-1.7B-GGUF Q8_0), downloaded into `tmp/hf` by the `cargo xtask e2e`
  suite that reads them and removed when it ends; the `candle` suite pulls its own checkpoints and GGUFs from the Hub
  through `ardana` and removes them. Never replace a real model with an invented fixture, never
  assert against a mock for an end-to-end requirement, never report a check you did not run.

## Guidelines

Read the guideline for every technology you touch and follow it. When a change alters how the stack is used (a new
pinned version, library, pattern or rule), update the matching guideline in the same commit.

- [Rust, cargo workspace, errors, testing, xtask](docs/guidelines/rust.md)
- [llama-cpp-2 and llama.cpp](docs/guidelines/llama-cpp.md)
- [Candle: safetensors causal LMs, architectures, Metal](docs/guidelines/candle.md)
- [Hugging Face tokenizers, hf-hub, cache layout, chat templates](docs/guidelines/huggingface.md)
- [axum and tower-http](docs/guidelines/axum.md)
- [Leptos CSR, trunk, wasm, CSS](docs/guidelines/leptos.md)
- [Playwright end-to-end tests](docs/guidelines/playwright.md)
- [Python tooling: uv venvs and tools](docs/guidelines/python-tooling.md)
- [ONNX browser variants: the model builder, the ardana-ai repositories, onnxruntime-web](docs/guidelines/onnx.md)
- [Fine-tuning: uv, the venv's pins, TRL and PEFT, the converter and the quantizer](docs/guidelines/finetune.md)
