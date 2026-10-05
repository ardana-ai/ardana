# ONNX guidelines

Covers the browser variants of library models: `cargo xtask onnx convert|publish <name>` (`xtask/src/onnx.rs`,
`xtask/scripts/onnx_export.py`), which export a Qwen3.5 checkpoint with the onnxruntime-genai model builder in the
`tmp/py/onnx` venv, convert the GGUF ardana-ai hosts with llama.cpp's `convert_hf_to_gguf.py`, write both into the
sandbox Hub cache and upload them to huggingface.co/ardana-ai; and onnxruntime-web, which runs the exported graphs in
a visitor's tab (the playground's in-tab engine, `crates/ardana-playground/src/engine.{rs,js}`, with its reader
`crates/ardana-engine`). Ardana's binary runs no ONNX: it serves the browser variants' files (`axum.md`, "Browser
variants").

## Versions
- `onnxruntime-genai` 0.17.1 — the model builder (`onnxruntime_genai.models.builder`) that exports the graphs; the PyPI
  release, newer than the newest git tag (v0.17.0).
- `onnxruntime-web` 1.30.0 — runs the exported graphs in the browser (WebGPU, else WASM), vendored in
  `crates/ardana-playground/ort/1.30.0/` (provenance in `ort/README.md`).
- `onnxruntime` 1.30.0 — the quantizer the builder calls (`onnxruntime.quantization`, k-quant).
- The rest of the `tmp/py/onnx` venv, on a uv-managed CPython 3.12: `onnx` 1.23.1, `onnx-ir` 1.0.0, `torch` 2.14.1,
  `transformers` 5.18.0, `tokenizers` 0.23.2, `safetensors` 0.8.0, `huggingface-hub` 1.33.0 (its `hf` CLI downloads
  checkpoints and uploads repositories), `numpy` 2.5.3, `ml-dtypes` 0.6.0, `protobuf` 7.36.2 (`xtask/src/onnx.rs`
  `PACKAGES`).
- llama.cpp `b11074` (`26394b4e6749a41c3633db040e0987500a5f7013`) — the release `llama-cpp-sys-2` 0.1.157 bundles, a
  research clone in `tmp/src/llama.cpp` (`xtask/fetch.toml`) for its `convert_hf_to_gguf.py`, `conversion/` package
  and `gguf-py`, which the crate's copy lacks.

## Rules

### The step
- The library document (`../ardana-landing/src/lib/data/models.json`, the landing checkout beside this one, read as
  `ardana` reads it) declares the work: `<name>` is a library name as `ardana` reads it that means a size's default
  quant (Q10: `decider:0.8b`, `Decider:0.8B`, `decider:0.8b-q8_0`; `decider` is its latest size), which names one
  size, its `source` (`{repo, commit}`, the checkpoint) and its `browser` table (`repo`
  `hf.co/ardana-ai/<repo>-ONNX`, `quant`). `cargo xtask onnx convert <name>` builds them and records the browser
  table's `commit` (the local snapshot's), `bytes` (what a browser downloads: `model.onnx`, `model.onnx.data`,
  `tokenizer.json`) and `profile` (what `GET /v1/browser/<name>/profile` answers, read from the snapshot by
  `BrowserCache::variant`); a size whose `gguf.repo` is an ardana-ai repository also gets its GGUF built, into the
  `file` its quant names, and its `gguf.commit` and that quant's `bytes` recorded. Never write these by hand; rerun the
  step. A name of another quant (`decider:2b-q8_0`), a tag the family lacks and a family the library lacks are refused,
  saying which; so is a size without a browser table, by name; and `convert` and `publish` stop, naming the document,
  when the landing checkout is not there.
- What the steps record changes those fields of the one size and no other byte: the document is read as a
  `serde_json` value and written back in the landing's Prettier formatting (tabs, width 100, every object one key a
  line, an array on one line where it fits and else numbers filling the lines), and a document that does not read
  back byte for byte in that formatting is refused rather than rewritten. The result goes to the document and to the
  library snapshot (`crates/ardana-registry/tests/data/models.json`), which stay byte-identical; `npm run lint` in
  the landing checks the formatting.
- Browser variants exist for decider:0.8b (int4, the browser default), decider:2b (int4) and qwen3.5:0.8b (int8: at
  int4 the spike saw 4 of its 6 preset answers flip); decider:4b and smollm3:3b stay native-only.
- The step runs under the home guard, by hand only: never in CI or from a test.

### Venv and downloads
- `tmp/py/onnx` is the step's own venv (`python::pinned_venv`, the `export-decider` pattern), recreated when
  `pyvenv.cfg` points outside `tmp/uv/python`, with the pins above. llama.cpp's converter runs in it too, with the
  clone's `gguf-py` (the script puts it first on `sys.path`); it converts decider:0.8b with transformers 5.18 although
  llama.cpp's requirements file pins 4.57.6.
- The checkpoint comes through `tmp/py/onnx/bin/hf download <repo> --revision <commit>` into `tmp/hf/hub` (a cached
  snapshot is read in place); the export and the conversion then run with `HF_HUB_OFFLINE=1` and
  `TRANSFORMERS_OFFLINE=1`.

### Export recipe
- Run the builder as its CLI does (`runpy` of `onnxruntime_genai.models.builder`): `-i <snapshot> -o <dir> -e webgpu
  -c <cache>` (fp16 inputs and outputs) with `--extra_options hf_token=false prune_lm_head=true exclude_mtp=true`:
  logits of the last position only, `[batch, 1, vocab]` (a decider row ends in its answer slot), and no MTP head.
- `int4`: `-p int4` and `algo_config=k_quant shared_embeddings=true`: 4-bit k-quant blocks of 32, the embedding read
  from the LM head's table through `GatherBlockQuantized`.
- `int8`: no `-p`, and `--target_options` whose `quant_config.weights` is the int4 k-quant base
  (`op_types: [MatMul, Gather]`) with int8 overrides: the presets `last_matmul`, `linear_attn` and `mixed_layers`, then
  each full-attention layer's `/model/layers.<n>/attn/{q,k,v,o}_proj/MatMul` and `/mlp/{gate,up,down}_proj/MatMul` by
  exact name. Every MatMul is then 8-bit and the embedding stays shared; `-p int8` stores the embedding apart (1.38 GB
  for a 0.8B model). Overrides take only a preset or an exact node name, and the first match wins.
- One monkeypatch, no fork: the builder dispatches only `Qwen3_5ForConditionalGeneration`, while decider checkpoints
  are the text-only `Qwen3_5ForCausalLM` (`qwen3_5_text`), so `AutoConfig.from_pretrained` reports that architecture
  for them and caps `max_position_embeddings` at 40,960, ardana-core's default context window (smaller position
  tables, identical answers). The weights still load as `Qwen3_5ForCausalLM`: the decoder's model type is the
  architecture name, which no class of the builder's loader map matches, so it calls `AutoModelForCausalLM`, whose
  own config read asks for its unused arguments and gets a tuple the patch leaves alone. The stock Qwen3.5-0.8B (its
  config is the vision-language one) runs the builder unpatched.
- The spike's second patch (`onnxruntime_genai.models.builders.base.Qwen3_5ForConditionalGeneration =
  AutoModelForCausalLM`) changed a module the builder never imports: `onnxruntime_genai/models/__init__.py` puts its
  directory on `sys.path` and the builder imports `builders.base`, a second module object. It is left out, and the
  exports are byte-identical to the spike's (decider:2b int4 and qwen3.5:0.8b int8, sha256).
- The graph: inputs `input_ids` (int64, `[batch, seq]`), `attention_mask` (int64, `[batch, total_seq]`),
  `position_ids` (int64, `[3, batch, seq]`, interleaved MRoPE), fp16 `past_key_values.<n>.{key,value}` for the
  full-attention layers (3, 7, ... 23) and `past.<n>.conv`, `past.<n>.recurrent` for the linear-attention ones; outputs
  `logits` (fp16, `[batch, 1, vocab]`) and the `present.*` states.
- The builder's own tokenizer files are not shipped: it re-saves a different `tokenizer.json` for the stock Qwen
  checkpoint, so the repositories carry the source's files and browser and native read the same ids.

### GGUF
- `convert_hf_to_gguf.py <snapshot> --outtype q8_0 --no-nextn --model-name <name> --outfile <file>`: the canonical
  name, and the `file` the document gives the size's default quant (`decider-0.8b-Q8_0.gguf`), which a library pull
  downloads by that name. The converter writes F32, F16, BF16 and Q8_0 itself;
  another library `quant` for an ardana-ai GGUF is refused.
- Always pass `--no-nextn`: decider configs declare `mtp_num_hidden_layers: 1` without MTP weights, and without the
  flag the GGUF claims 25 blocks and `nextn_predict_layers = 1`, which llama.cpp refuses to load ("null result from
  llama cpp"). Ardana never drafts, so no GGUF needs the draft tensors.

### Repositories and the cache layout
- `ardana-ai/<repo>-ONNX` holds `model.onnx`, `model.onnx.data` and `README.md`; `ardana-ai/<repo>-GGUF` the GGUF and
  `README.md`. Both carry the source's `tokenizer.json`, `tokenizer_config.json`, `chat_template.jinja`,
  `decider_config.json` and `LICENSE`, unchanged, where it has them.
- `convert` writes each repository into `tmp/hf/hub/models--ardana-ai--<repo>/` in the official layout
  (`huggingface.md`, "Hub cache layout"): `blobs/<name>`, `snapshots/<commit>/<file>` linking relatively into
  `blobs/`, `refs/main` holding the commit. A blob is named by its sha256 when the Hub stores the file with Git LFS
  (`*.onnx` in its default `.gitattributes`, or larger than 10 MB), else by its git blob id, as `cargo xtask fetch`
  names a download, so a later fetch of the published repository reuses the blobs.
- The commit is local until `publish`: the git blob id of the snapshot's manifest (a `<blob> <file>` line per file,
  sorted), so rebuilding the same files makes the same snapshot, and `convert` records it in the document. After
  `publish`, `[[hf]]` and the document pin the Hub's commit and `cargo xtask fetch` moves `refs/main` to it.
- `convert` reads both back offline through `ardana_registry`'s Hub: the release `ardana pull <name> --tokenizer
  hf.co/ardana-ai/<repo>-ONNX` (the size's `browser.repo`) in the scratch home `tmp/onnx/<name>/home`, with
  `HF_HUB_OFFLINE=1` and `ARDANA_LIBRARY` at the document it just recorded, whose registry entry must point into the
  snapshots it wrote.

### Model cards
- Front matter `license` (the source card's), `base_model: <source repo>`, `base_model_relation: quantized` and
  `tags: [ardana, onnx|gguf]`; then the source repository and revision, the command that built it, the size's
  canonical name with its `ardana pull <family>:<size>` and `ardana run <family>:<size>` lines, and what each file is.
  Claim nothing the step did not do.

### onnxruntime-web
- Vendored from the npm tarball `onnxruntime-web-1.30.0.tgz`, checked against the registry's `dist.integrity`, with the
  repository's MIT `LICENSE` and `ThirdPartyNotices.txt` at `v1.30.0` beside them; the page imports them from its own
  origin (`/ort/1.30.0/`), never from a CDN. Two builds: `ort.webgpu.bundle.min.mjs` with
  `ort-wasm-simd-threaded.asyncify.wasm` for the `webgpu` execution provider, and `ort.wasm.bundle.min.mjs` with
  `ort-wasm-simd-threaded.wasm` for `wasm`. The WebGPU build's WASM module has no CPU kernel for
  `com.microsoft.GatherBlockQuantized`, the shared int4 embedding (`shared_embeddings=true`), so its `wasm` provider
  refuses these graphs ("Failed to find kernel"); the plain build runs them. Each bundle loads its WASM module from
  beside itself (`import.meta.url`).
- Import a build on first use (`engine.js#runtime`), never at page load: a page that only runs server models loads no
  runtime. Picking an "In browser" row starts the import, the page's own opening pick of one included (an empty
  registry). A failed import is not kept: the next run imports the build again, under a `?retry=<n>`
  query, since a browser may keep a module's failed fetch for its URL (the build still loads its WASM module from beside
  itself, `import.meta.url` without the query). A build whose WASM module could not be fetched is forgotten too
  (`createSession` fails with `NotLoaded`, recognised by the vendored Emscripten loader's "fetching of the wasm failed"):
  onnxruntime-web caches a backend's failed start for the life of its module, so the next run imports the build afresh,
  and a WASM session that answered in its place is released after that run rather than kept. A build that loaded and
  could not start the model (a GPU that cannot run it) keeps that failure, and its WASM stand-in is kept: nothing
  imports onnxruntime-web again on every run. Set `env.logLevel = 'error'` and the session's
  `logSeverityLevel: 3`: the builder's graphs place shape operators on the CPU on purpose, which onnxruntime otherwise
  reports as a warning on every session.
- WebGPU, else WASM: a browser with `navigator.gpu` gets a `webgpu` session, and one whose WebGPU session fails, or
  without `navigator.gpu`, a `wasm` one; the page says which (`in this tab on WebGPU`). The WASM backend runs on
  threads only in a cross-origin isolated page (`crossOriginIsolated`), which the server's COOP and COEP headers make
  every page (`axum.md`, Q13); the bundles' worker scripts come from the same origin.
- `InferenceSession.create(<model.onnx bytes>, { executionProviders: [device], externalData: [{ path:
  'model.onnx.data', data: <bytes> }] })`: the tab passes both files as `Uint8Array`s it downloaded or kept, never a
  URL, so onnxruntime fetches nothing itself. `session.release()` frees the weights before another model loads, and when
  the page picks a server row (the next run in the tab starts a session again from the kept files). It frees the
  session's GPU buffers (about 625 MB of the GPU process's memory for decider:0.8b on WebGPU, 1.2 GB for qwen3.5:0.8b,
  1.4 GB for decider:2b); the page's WebAssembly memories (onnxruntime-web's and the engine module's) never shrink, so
  their high mark (on WebGPU about 585 MB for decider:0.8b, 895 MB for qwen3.5:0.8b and 966 MB for decider:2b; on
  WASM 1.94, 2.44 and 3.87 GB, under wasm32's 4 GB) stays with the page until a reload, and a later session reuses it.
  With the GPU process's share (1.0, 2.1 and 2.3 GB on WebGPU), a run takes 2.7 to 4.1 times the browser files in
  memory, which the page says as "3 to 4 times" beside what a first run downloads (measured in Chrome 154 with
  `performance.measureUserAgentSpecificMemory` after forced collections and macOS `footprint`). A release waits for
  the row the session is decoding (`engine.js` keeps each session's `session.run` promise): onnxruntime-web frees a
  session at once, even while a row of it waits on the GPU, and that row then reads freed memory (measured: it failed
  with "failed to call OrtRun(). ERROR_CODE: 2, ERROR_MESSAGE: input name cannot be empty" when a server row was picked
  mid-row).
- One `session.run(feeds, ['logits'])` per planned row, from position 0 with empty caches: `input_ids` (int64
  `[1, n]`), `attention_mask` (int64 ones `[1, n]`), `position_ids` (int64 `[3, 1, n]`, `0..n` on each MRoPE axis),
  and every other input a zero tensor of its metadata type and shape with `batch_size` 1, `past_sequence_length` 0
  and `kv_cache_dim` 256 (the exported caches' head width); an unknown symbolic dimension is an error. The logits are
  fp16 (`Uint16Array`, decoded exactly to f32) of the last position, read at the row's label ids; the engine module
  (`crates/ardana-engine`) replays them through `ardana_core::Decider::run`.

### Publish and the token (Q12)
- `publish` prints, for each repository, the card, the `hf repos create ardana-ai/<repo> --type model --public
  --exist-ok` and `hf upload ardana-ai/<repo> <snapshot> . --type model --commit-message <message> --format quiet`
  commands and the `[[hf]]` entry it pins, then the size's document entry it pins; `--dry-run` stops there and writes
  nothing.
- The token is the user's: `HF_TOKEN`, else the real home's `~/.cache/huggingface/token` (`hf auth login`), read by
  xtask's own code, since the sandbox `HF_HOME` holds none. A child gets it only as `HF_TOKEN`, never in argv, and
  nothing prints it: `publish` never formats a `Command` with `{:?}`, whose output lists the variables set on it.
- A failed upload blocks nothing: `publish` exits non-zero after the printed commands, and the local repositories
  and the library document stay valid offline. Rerun it, or run the commands by hand.
- After an upload, `publish` pins `[[hf]]` (`repo`, the Hub commit `hf upload` prints, every file but `README.md` and
  `LICENSE`) in `xtask/fetch.toml`, replacing an older pin of the repository, and the same commit in the document and
  the snapshot: the browser table's `commit` for the ONNX repository, the size's `gguf.commit` for the GGUF one.
  `cargo xtask fetch --tests` leaves out `.onnx`, `.onnx.data` and `.gguf` files, so CI downloads no weights.

## Sources
- https://onnxruntime.ai/docs/genai/howto/build-model.html — the model builder: inputs, precisions, execution providers
- https://github.com/microsoft/onnxruntime-genai/blob/main/src/python/py/models/README.md — builder options: `prune_lm_head`, `shared_embeddings` with `algo_config=k_quant`, `exclude_mtp`, `target_options`
- https://pypi.org/project/onnxruntime-genai/0.17.1/ — the pinned builder release
- https://pypi.org/project/onnxruntime/1.30.0/ — the pinned quantizer release
- https://onnxruntime.ai/docs/tutorials/web/ — onnxruntime-web
- https://onnxruntime.ai/docs/tutorials/web/ep-webgpu.html — the WebGPU execution provider
- https://onnxruntime.ai/docs/tutorials/web/large-models.html — external data (`model.onnx.data`) and browser limits
- https://onnxruntime.ai/docs/tutorials/web/env-flags-and-session-options.html — `env.wasm.numThreads` (cross-origin isolation), `env.logLevel`, `logSeverityLevel`
- https://onnxruntime.ai/docs/tutorials/web/build-web-app.html — the bundles, their WASM files and serving them from the app's origin
- https://www.npmjs.com/package/onnxruntime-web — the onnxruntime-web package
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/convert_hf_to_gguf.py — the GGUF converter at the pin (`--outtype`, `--no-nextn`, `--model-name`)
- https://github.com/ggml-org/llama.cpp/releases/tag/b11074 — the pinned llama.cpp release
- https://huggingface.co/docs/huggingface_hub/guides/cli — `hf download`, `hf repos create`, `hf upload`
- https://huggingface.co/docs/huggingface_hub/package_reference/environment_variables — `HF_TOKEN`, `HF_HUB_OFFLINE`
- https://huggingface.co/docs/huggingface_hub/guides/manage-cache — the Hub cache layout
- https://huggingface.co/docs/hub/repositories-getting-started — files over 10 MB are stored with Git LFS (Xet)
- https://huggingface.co/docs/hub/model-cards — card metadata: `license`, `base_model`, `base_model_relation`
