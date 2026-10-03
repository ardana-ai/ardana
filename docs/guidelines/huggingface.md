# Hugging Face guidelines

Covers the Hugging Face side of Ardana: the `tokenizers` crate (`tokenizer.json`) in `crates/ardana-core`, natively
and in the wasm32 build of the playground's engine module (`crates/ardana-engine`), chat template rendering with
`minijinja` and `minijinja-contrib` in `crates/ardana-core/src/chat.rs`, the `hf-hub` client and the Hub cache layout
in `crates/ardana-registry` (`hf.co/` refs, `ardana pull`, the browser variants `ardana serve` pulls), and the
`tmp/hf` model store that `cargo xtask fetch` fills from `xtask/fetch.toml`.

## Versions
- `tokenizers` 0.23.2 — loads each model's HF `tokenizer.json`; the single source of prompt token ids (Q2).
- `hf-hub` 1.0.0 — async Hub client used by `ardana pull` and registry resolution; reads the standard hub cache.
- `minijinja` 2.24.0 — renders HF chat templates (Jinja2 syntax) to derive chat-layout head and tail ids (W3).
- `minijinja-contrib` 2.24.0 — `pycompat` feature only, for the Python string/dict methods HF templates call.

## Rules

### Sandbox and cache location
- Every Hub file Ardana, its tests or xtask touch lives under `tmp/hf` (`HF_HOME=tmp/hf`, hub at `tmp/hf/hub`);
  `.cargo/config.toml` `[env]` forces this for every cargo-run process. Never hard-code `~/.cache/huggingface`.
- Build the client with `HFClient::new()` (or `HFClient::builder().build()`), which resolves the cache once at build
  time: `HF_HUB_CACHE`, then `HUGGINGFACE_HUB_CACHE`, then `$HF_HOME/hub`, then `$XDG_CACHE_HOME/huggingface/hub`,
  then `~/.cache/huggingface/hub`. Tests that need another root pass `.cache_dir(..)` explicitly under `$ARDANA_TMP`.
- hf-hub reads only `HOME` for that last step and falls back to `/tmp` without it (as on Windows), so
  `hub::Hub::from_env` passes `.cache_dir(<home>/.cache/huggingface/hub)` from `std::env::home_dir()` when none of
  `HF_HUB_CACHE`, `HUGGINGFACE_HUB_CACHE`, `HF_HOME`, `XDG_CACHE_HOME` and `HOME` is set; every variable keeps its
  precedence. The Ardana home and the Ollama store fall back to `home_dir()` the same way.
- `HFClient` wraps an `Arc` and is `Clone + Send + Sync`: build one per process and clone it; never rebuild per file.
- Only `cargo xtask fetch` reads the real `~/.cache/huggingface/hub`, and only for `[[hf_local]]` entries it copies
  read-only into `tmp/hf/hub`: it resolves `snapshots/<revision>/<file>` to its blob, copies the blob into
  `tmp/hf/hub/.../blobs/`, verifies it against its name (git blob id, or LFS sha256 for 64-hex names), links the
  snapshot and writes `refs/main`. Ardana itself never reads or writes the user's real HF cache. The one other read
  of the real cache is `cargo xtask onnx publish`'s, of `~/.cache/huggingface/token` (`onnx.md`).
- `cargo xtask fetch --tests` installs only the files plain `cargo test` reads: every `[[hf]]` file (the library's
  models) but the weights (`.gguf`, `.onnx`, `.onnx.data`), which needs no token, so CI runs it. Plain tests use only the library's models
  (decider-2b, Qwen3.5, SmolLM3); a test that needs the gated `[[hf_local]]` Llama 3.2 tokenizer is a real-model
  test (`#[ignore = "e2e: ..."]`), like those on Ollama's `llama3.2`.
- `[[hf]]` entries in `xtask/fetch.toml` pin `repo`, a full commit `revision` and `files`. `cargo xtask fetch` lists
  the revision through `https://huggingface.co/api/models/<repo>/revision/<rev>?blobs=true`, downloads each file from
  `resolve/<rev>/<file>` with `curl` into `blobs/<LFS sha256 or git blob id>`, verifies that hash (`shasum -a 256`,
  `git hash-object`), links `snapshots/<rev>/<file>` relatively and writes `refs/main` with the pinned revision, so an
  offline `main` lookup resolves to the pinned snapshot. No `hf-hub` in xtask: it keeps xtask small.
- `ardana rm` deletes only the registry entry; never delete, prune or rewrite files in the hub cache.

### Downloading with hf-hub
- Download one file with
  `client.model(owner, repo).download_file().filename(name).revision(rev).local_files_only(offline).send().await`;
  it returns `HFResult<PathBuf>`, the path of `snapshots/<commit>/<name>` in the cache. Store that path in
  `models.toml`; it is a symlink into `blobs/`, so open it, never copy or rename the blob.
- Omit `.revision(..)` only when `main` is intended (it is the default); pin commits in `xtask/fetch.toml`.
- Never set `.local_dir(..)`: it downloads outside the cache layout and bypasses caching.
- Match on `HFError::LocalEntryNotFound` (cache miss with `local_files_only(true)`) separately from
  `HFError::EntryNotFound` (the file is not in the repo) and transport errors, and word each error differently.
- Use the async API from the existing tokio runtime; do not enable the `blocking` feature or build a second runtime.
- Never pass or log a token. `hf-hub` picks up `HF_TOKEN`, `HF_TOKEN_PATH` or `$HF_HOME/token` on its own; with
  `HF_HOME=tmp/hf` no token file exists, and every Step 1 repo is ungated.

### Model library
- `crates/ardana-registry/src/library.toml` (embedded, parsed once by `library::library()`, validated by a unit test)
  maps short names to `hf.co/` GGUF repositories: `name`, `weights`, the default `quant`, its GGUF `size` in bytes,
  `release_date`, and the `tokenizer` repository and `layout` when the GGUF repository needs them. A new model family
  is one `[[model]]` entry; read sizes and files from the Hub API (`/api/models/<repo>/tree/main`), never invent them.
- A model may name its `source`, the checkpoint `hf.co/<org>/<repo>@<40-hex commit>` that `cargo xtask onnx convert`
  builds from, and a `[model.browser]` table, its browser variant (`library::BrowserWeights`): `weights`
  (`hf.co/ardana-ai/<name>-ONNX`), `quant` (`int4` or `int8`) and the `size` a browser downloads (`model.onnx`,
  `model.onnx.data`, `tokenizer.json`), which the parser requires. `browser_default` names a model with one. The
  browser sizes, and the `size` of a GGUF ardana-ai hosts (decider-0.8b's), are the bytes `cargo xtask onnx convert`
  built and recorded (`onnx.md`).
- `ardana_registry::pull` resolves a library name first: `<name>[:<quant>]`, both case-insensitive, pulls
  `<weights>:<quant>` under the registry name `<name>` (or `<name>:<quant>`, lowercased, for another quant), with the
  library's tokenizer, layout and release date wherever the flags and the weights give none. Everything else is a
  reference (`Ref::parse`), whose error lists the library names.
- Library repositories are read at their `main` commit like any `hf.co/` ref; `xtask/fetch.toml` pins the commits the
  offline tests read from `tmp/hf`.

### Download progress
- `PullOptions::progress` attaches an `hf_hub::progress::ProgressHandler` per downloaded file (`hub::FileReport`,
  through the download builder's `maybe_progress`) that prints `downloading <file>: <bytes> / <size>` to stderr at
  most once a second while the count moves, and `<size> done` at the end. `DownloadEvent::Progress` carries per-file
  deltas on the HTTP path, `AggregateProgress` the totals of a xet transfer; a cache hit moves no bytes and prints
  nothing. Handlers must never block: the library calls them on the download's read path.
- hf-hub locks a blob while it downloads it (`cache::acquire_lock`), so two processes pulling the same file (the CLI
  and `serve`) do not write it twice. Xet keeps its own state under `$HF_HOME/xet`.

### Offline mode
- Read `HF_HUB_OFFLINE` in `ardana-registry` yourself (`ardana_registry::hub::offline`): treat `1`, `ON`, `YES`,
  `TRUE` (case-insensitive) as true, like the Python client, and pass `.local_files_only(true)` on every download call
  when it is set.
- In offline mode never call metadata endpoints (`info`, `list_tree`, `file_exists`); resolve everything through
  `local_files_only(true)`: `snapshot_download().local_files_only(true)` returns the cached `main` snapshot directory
  (its name is the commit), whose files stand for the repository's file list, and
  `download_file().revision(<commit>).local_files_only(true)` returns each file.
- A browser variant is resolved cache first (`ardana_registry::pull_browser`): `Hub::cache_only` reads it as offline
  mode would, and any error of that lookup pulls it from the Hub (unless `HF_HUB_OFFLINE` is set, when the error
  stands). `BrowserCache::holds` answers, for `x_browser_pulled`, whether the cache holds a variant whole: the same
  lookup of every file the pull reads (`browser_paths`), files found and not read. A variant `ardana serve` pulled once,
  or `cargo xtask onnx convert` built, is then served without a Hub call, by a server that needs no variable for it (its
  in-process tests cannot set one). Like offline mode, it takes the cached snapshot's files for the repository's, with
  one check: a library model whose entry names no `layout` reads its profile from `decider_config.json`, so a cached
  snapshot without that file is incomplete (`NotCached`), never read as the stock profile in the chat layout. The unit
  test `cached_browser_variants_are_complete` builds `Hub::new` over a scratch cache under `$ARDANA_TMP`, with a closed
  port as the Hub endpoint.

### Browser variants (`ardana-registry`)
- `pull_browser(name, progress)` resolves the `[model.browser]` table of the library model named exactly `name`: the
  ONNX repository's `model.onnx`, `model.onnx.data` and `tokenizer.json` (`BROWSER_FILES`), with the tokenizer's
  `tokenizer_config.json` and `chat_template.jinja` and the repository's `decider_config.json` when it has them, all at
  one commit, into the hub cache only; a browser variant is never a registry entry. The profile is the one `ardana
  pull <name>` derives: `decider_config.json` (plain layout) or the stock profile named `name` in the library's layout,
  dated by the library's release date. `BrowserModel` holds the files, the commit and the profile.

### `ardana pull` (`ardana-registry`)
- Online, list a repository once with `model(org, repo).info().send()` (`sha` is the `main` commit, `siblings` the
  files) and download every file at that commit, so the files of one pull come from one commit.
- `hf.co/<org>/<repo>:<quant>` picks the one GGUF whose stem ends in `-<quant>` or `.<quant>` case-insensitively
  (`Q4_K_M` without a quant, Ollama's `hf.co/` default); none or several is an error listing the repository's GGUFs.
  `hf.co/<org>/<repo>:<file>.gguf` names the file.
- A tokenizer repository contributes `tokenizer.json` plus `tokenizer_config.json` and `chat_template.jinja` when it
  has them, all at one commit, so the chat template sits next to the tokenizer in `snapshots/<commit>/`.
- `decider_config.json` in the weights repository gives the profile (plain layout); without it the model gets
  `ModelProfile::stock` named after the entry, in the chat layout. `--tokenizer`, `--name` and `--layout` override.
- Record snapshot paths and local paths absolute but unresolved (`std::path::absolute`, never `canonicalize`): a
  snapshot path resolves to `blobs/<hash>`, whose directory holds neither the chat template nor `decider_config.json`.
- A missing repository answers 401 "Authentication required" without a token, like a gated one; report the `hf.co/`
  ref with the error.

> Plan note: `hf-hub` 1.0.0 does not read `HF_HUB_OFFLINE` (its env constants are `HF_ENDPOINT`, `HF_TOKEN`,
> `HF_TOKEN_PATH`, `HF_HOME`, `HF_HUB_CACHE`, `HUGGINGFACE_HUB_CACHE`, `XDG_CACHE_HOME`,
> `HF_HUB_DISABLE_IMPLICIT_TOKEN`, `HF_HUB_USER_AGENT_ORIGIN`); only `local_files_only` is built in, so Ardana
> must map the variable itself (R4.1).

### Hub cache layout
- The layout under `tmp/hf/hub` is the official one: `models--<org>--<repo>/` holding `blobs/<hash>` (file bodies),
  `snapshots/<commit>/<path>` (symlinks into `blobs/`) and `refs/<revision>` (a text file with the commit id, e.g.
  `refs/main`). Python tooling may also add `.no_exist/` and `trees/`; ignore them.
- Any code that writes into the cache without `hf-hub` (the `[[hf_local]]` copy for Q18, the repositories
  `cargo xtask onnx convert` builds before they are published) must produce exactly this shape, including `refs/main`
  holding the snapshot commit, so a `main` lookup resolves offline. A repository that exists only locally gets a local
  commit id (`onnx.md`, "Repositories and the cache layout").
- Treat everything in the cache as read-only once written; a changed file means a new commit, never an edit.

### Gated repositories (Q18)
- Never download from a gated repo (meta-llama/*): the Hub requires an approved access request and an authenticated
  token. The `meta-llama/Llama-3.2-3B-Instruct` tokenizer files come only from the user's existing HF cache via the
  `[[hf_local]]` entry, copied read-only into `tmp/hf/hub`, and `ardana pull` resolves them with
  `local_files_only(true)`.
- Prefer ungated sources for any new target (ggml-org GGUFs, Qwen/Qwen3.5-0.8B, HuggingFaceTB/SmolLM3-3B tokenizers).

### Tokenizers
- Depend on `tokenizers` with `default-features = false` and one regex backend per target: `ardana-core` takes
  `onig` natively (the engine the Python package uses for `Split` pre-tokenizers, Qwen's) and `unstable_wasm`
  (`fancy-regex`, pure Rust, and `getrandom`'s `wasm_js`) on wasm32, where Oniguruma's C does not compile, through
  `[target.'cfg(...)'.dependencies]` tables (resolver 3 leaves a target's features out of the others'). The workspace
  entry names no backend; `ardana-registry` and `ardana`, native only, name `onig` themselves. The default
  `progressbar` and `esaxx_fast` only serve training. Both backends read decider's prompt goldens to the same ids.
- Load with `Tokenizer::from_file(path)`; map its `Box<dyn Error + Send + Sync>` into a typed error naming the path.
  `Tokenizer` is `Clone + Send + Sync`: load once per model and share it. Loading decider-2b's 20 MB
  `tokenizer.json` takes about a second in a debug build, so tests load it once and clone.
- Encode with `tokenizer.encode_fast(text, false)` (`encode` without offsets): the prompt builder and the chat
  head/tail own every special token, and `true` would let the post-processor add its own (a double BOS). Assert the
  head ids in tests (R3.1, R3.3).
- transformers 5.17 builds decider-2b's `Qwen2Tokenizer` from `tokenizer.json`'s pre-tokenizer and ignores the
  differing `pretokenize_regex` in its `tokenizer_config.json` (checked: equal ids on text with combining marks), so
  `tokenizer.json` alone reproduces decider's ids.
- Keep ids as `u32` end to end (`Encoding::get_ids() -> &[u32]`); convert to llama.cpp's `i32` only inside
  `ardana-llama`.
- Look up label ids with `token_to_id` and check each label encodes to exactly one token (`labels_single_token`).
- Use `get_vocab_size(true)` (with added tokens) when comparing against model logits width; see
  `docs/guidelines/llama-cpp.md` for the Qwen3.5 248,320 vs 248,070 mismatch.
- Keep the `http` feature off: it adds `Tokenizer::from_pretrained` and a second, older `hf-hub` with its own
  download path. All downloads go through `ardana-registry`.

### Chat templates
- Source order: `chat_template.jinja` next to the tokenizer, else the `chat_template` string in
  `tokenizer_config.json` (the same precedence transformers uses); neither present is the "has no chat template"
  error (R3.4). A list-valued `chat_template` (named templates) is an error naming the file.
- Read `bos_token` and `eos_token` from `tokenizer_config.json` into `TemplateSpecials`; fail clearly when a value is
  not a string instead of rendering an empty token.
- Render with a `minijinja::Environment::new()` configured like transformers' sandbox: `set_trim_blocks(true)`,
  `set_lstrip_blocks(true)`, the minijinja `loop_controls` feature enabled (for `{% break %}`/`{% continue %}`), and
  `env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback)`.
- `pycompat` supplies str `strip`, `lstrip`, `rstrip`, `startswith`, `endswith`, `split`, `replace`, `lower`,
  `upper`, `format` and more, dict `get`, `items`, `keys`, `values`, and list `count`; a template calling anything
  else fails at render time, so add a test per shipped template.
- Register `raise_exception(msg)` as a function that returns a minijinja error carrying `msg`.
- Register `strftime_now(format)` on the fixed date 2024-07-26 (`ardana_core::chat::TEMPLATE_DATE`), never the
  clock, so every head is identical every day (R3.3): Llama 3.2 renders "26 Jul 2024" (its own fallback date), and
  SmolLM3, whose template calls `strftime_now` unguarded, renders "26 July 2024". The formatter supports `%d`, `%b`,
  `%B`, `%Y` and `%%`; any other directive is a render error naming it, so a new template using one needs a test.
- Keep the default `UndefinedBehavior::Lenient` (undefined prints empty and is falsy), which matches how HF templates
  probe optional variables.
- Render one user message holding the sentinel with `add_generation_prompt=true`, `tools` and `documents` set to
  `none` (as `apply_chat_template` passes them), `bos_token`/`eos_token` only when set (a `null` token stays
  undefined, as in transformers' `special_tokens_map`); pass `enable_thinking=false` only when the template text
  mentions it (decider `ChatTemplate`).
- transformers' `{% generation %}` .. `{% endgeneration %}` tag (SmolLM3's template) only marks assistant text and
  renders its body; minijinja has no custom tags, so `ardana_core::chat` rewrites each into `{% if true %}` ..
  `{% endif %}` with its whitespace control kept before compiling.
- decider (through transformers) defines `strftime_now` with the real clock, so its dated heads differ from Ardana's
  by the date only; checked with transformers 5.17, Ardana's head and tail ids equal decider's for Qwen3.5-0.8B and,
  given the same date, for SmolLM3-3B.
- Do not rely on `tojson` for byte-exact output: minijinja's `tojson` needs the `json` feature and escapes HTML
  characters, unlike transformers' `json.dumps(ensure_ascii=False)`.

## Sources
- https://docs.rs/tokenizers/0.23.2/tokenizers/index.html — crate overview, `http`/`onig`/`fancy-regex` features
- https://docs.rs/tokenizers/0.23.2/tokenizers/tokenizer/struct.TokenizerImpl.html#method.encode_fast — `encode_fast`
- https://huggingface.co/docs/hub/api — `/api/models/{repo}/revision/{revision}`, sibling `blobId`, `lfs.sha256`, `size`
- https://docs.rs/tokenizers/0.23.2/tokenizers/tokenizer/struct.Tokenizer.html — `from_file`, `encode(.., add_special_tokens)`, `token_to_id`, `get_vocab_size`, Send/Sync
- https://docs.rs/tokenizers/0.23.2/tokenizers/tokenizer/struct.Encoding.html — `get_ids() -> &[u32]`
- https://docs.rs/tokenizers/0.23.2/tokenizers/tokenizer/struct.AddedToken.html — special/added token flags
- https://huggingface.co/docs/tokenizers/pipeline — normalization to post-processing, where special tokens are added
- https://docs.rs/hf-hub/1.0.0/hf_hub/index.html — `HFClient`, download chain, env vars, `blocking` feature
- https://docs.rs/hf-hub/1.0.0/hf_hub/struct.HFClient.html — `new`, `model(owner, name)`, Clone/Send/Sync
- https://docs.rs/hf-hub/1.0.0/hf_hub/struct.HFClientBuilder.html — `cache_dir` resolution order, `build`
- https://docs.rs/hf-hub/1.0.0/hf_hub/repository/struct.HFRepository.html — `download_file`, `local_files_only`, `LocalEntryNotFound`
- https://docs.rs/hf-hub/1.0.0/hf_hub/repository/download/struct.HFRepositoryDownloadFileBuilder.html — setters, `send() -> HFResult<PathBuf>`
- https://docs.rs/hf-hub/1.0.0/src/hf_hub/constants.rs.html — env constants (no `HF_HUB_OFFLINE`), `hf_home`, `resolve_cache_dir`
- https://docs.rs/hf-hub/1.0.0/hf_hub/cache/index.html — blobs/snapshots layout as seen by hf-hub
- https://huggingface.co/docs/huggingface_hub/guides/manage-cache — official cache layout: refs, blobs, snapshots
- https://huggingface.co/docs/huggingface_hub/package_reference/environment_variables — `HF_HOME`, `HF_HUB_CACHE`, `HF_HUB_OFFLINE`, boolean values
- https://huggingface.co/docs/hub/models-gated — gated repos need approved access and a token
- https://huggingface.co/docs/transformers/chat_templating — `add_generation_prompt`, avoid duplicate special tokens
- https://huggingface.co/docs/transformers/chat_templating_writing — `chat_template.jinja` vs `tokenizer_config.json`, `raise_exception`, `strftime_now`
- https://github.com/huggingface/transformers/blob/main/src/transformers/utils/chat_template_utils.py — trim_blocks, lstrip_blocks, loopcontrols
- https://docs.rs/minijinja/latest/minijinja/ — crate overview and feature list (`loop_controls`, `json`)
- https://docs.rs/minijinja/2.24.0/minijinja/struct.Environment.html — `set_unknown_method_callback`, trim/lstrip defaults, `add_function`
- https://docs.rs/minijinja/2.24.0/minijinja/enum.UndefinedBehavior.html — Lenient default semantics
- https://docs.rs/minijinja/2.24.0/minijinja/filters/fn.tojson.html — `json` feature, HTML-safe escaping
- https://docs.rs/minijinja-contrib/2.24.0/minijinja_contrib/pycompat/fn.unknown_method_callback.html — pycompat registration and method list
