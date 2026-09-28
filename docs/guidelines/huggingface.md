# Hugging Face guidelines

Covers the Hugging Face side of Ardana: the `tokenizers` crate (`tokenizer.json`) in `crates/ardana-core`, chat
template rendering with `minijinja` and `minijinja-contrib` in `crates/ardana-core/src/chat.rs`, the `hf-hub` client
and the Hub cache layout in `crates/ardana-registry` (`hf.co/` refs, `ardana pull`), and the `tmp/hf` model store
that `cargo xtask fetch` fills from `xtask/fetch.toml`.

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
- `HFClient` wraps an `Arc` and is `Clone + Send + Sync`: build one per process and clone it; never rebuild per file.
- Only `cargo xtask fetch` reads the real `~/.cache/huggingface/hub`, and only for `[[hf_local]]` entries it copies
  read-only into `tmp/hf/hub`: it resolves `snapshots/<revision>/<file>` to its blob, copies the blob into
  `tmp/hf/hub/.../blobs/`, verifies it against its name (git blob id, or LFS sha256 for 64-hex names), links the
  snapshot and writes `refs/main`. Ardana itself never reads or writes the user's real HF cache.
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

### Offline mode
- Read `HF_HUB_OFFLINE` in `ardana-registry` yourself: treat `1`, `ON`, `YES`, `TRUE` (case-insensitive) as true,
  like the Python client, and pass `.local_files_only(true)` on every download call when it is set.
- In offline mode never call metadata endpoints (`info`, `list_tree`, `file_exists`); resolve everything through
  `download_file().local_files_only(true)`.

> Plan note: `hf-hub` 1.0.0 does not read `HF_HUB_OFFLINE` (its env constants are `HF_ENDPOINT`, `HF_TOKEN`,
> `HF_TOKEN_PATH`, `HF_HOME`, `HF_HUB_CACHE`, `HUGGINGFACE_HUB_CACHE`, `XDG_CACHE_HOME`,
> `HF_HUB_DISABLE_IMPLICIT_TOKEN`, `HF_HUB_USER_AGENT_ORIGIN`); only `local_files_only` is built in, so Ardana
> must map the variable itself (R4.1).

### Hub cache layout
- The layout under `tmp/hf/hub` is the official one: `models--<org>--<repo>/` holding `blobs/<hash>` (file bodies),
  `snapshots/<commit>/<path>` (symlinks into `blobs/`) and `refs/<revision>` (a text file with the commit id, e.g.
  `refs/main`). Python tooling may also add `.no_exist/` and `trees/`; ignore them.
- Any code that writes into the cache without `hf-hub` (the `[[hf_local]]` copy for Q18) must produce exactly this
  shape, including `refs/main` holding the snapshot commit, so a `main` lookup resolves offline.
- Treat everything in the cache as read-only once written; a changed file means a new commit, never an edit.

### Gated repositories (Q18)
- Never download from a gated repo (meta-llama/*): the Hub requires an approved access request and an authenticated
  token. The `meta-llama/Llama-3.2-3B-Instruct` tokenizer files come only from the user's existing HF cache via the
  `[[hf_local]]` entry, copied read-only into `tmp/hf/hub`, and `ardana pull` resolves them with
  `local_files_only(true)`.
- Prefer ungated sources for any new target (ggml-org GGUFs, Qwen/Qwen3.5-0.8B, HuggingFaceTB/SmolLM3-3B tokenizers).

### Tokenizers
- Depend on `tokenizers` with `default-features = false, features = ["onig"]`: `onig` is the regex engine the Python
  package uses for `Split` pre-tokenizers (Qwen's), while the default `progressbar` and `esaxx_fast` only serve
  training.
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
