# llama.cpp guidelines

Covers the `llama-cpp-2` crate (utilityai/llama-cpp-rs) and the llama.cpp library it builds, as used by
`crates/ardana-llama`, the only crate that may depend on them. `ardana-llama` implements the `ardana-core` traits
`Runtime` and `LoadedModel` (`load(weights, &LoadOptions)`, `slot_logits(ids, slots, label_ids)`); the `ardana`
binary registers it in `Runtimes`. Tokenization, prompt building and readout stay in `ardana-core` and never call
llama.cpp.

## Versions
- `llama-cpp-2` 0.1.157 — safe, near-raw bindings to llama.cpp; its `llama-cpp-sys-2` dependency bundles llama.cpp
  at commit `26394b4` (`26394b4e6749a41c3633db040e0987500a5f7013`), which knows the `qwen35` architecture. Pin it
  with `=0.1.157`: the crate tracks llama.cpp closely and does not follow semver meaningfully.

## Rules

### Build
- Keep the dependency in `ardana-llama` only (`llama-cpp-2 = { workspace = true }`); `cargo xtask check-deps`
  rejects it anywhere else, and `ardana-server` must never see it.
- Declare it with `default-features = false`: the defaults add `openmp` (no OpenMP in Apple clang; the macOS build
  had it off anyway), `common` (llama.cpp's `common` library, which Ardana does not call) and an Android-only feature.
- Expect a from-source C/C++ build: `llama-cpp-sys-2` drives llama.cpp's CMake through the `cmake` crate, so `cmake`
  (4.4.3 on the dev machine) and a C/C++ toolchain must be on `PATH`. Build output stays under the repo's `target/`.
- Pass llama.cpp build options through `CMAKE_*` or `GGML_*` environment variables (the build script forwards both);
  never patch the vendored sources.
- x86_64 builds target the AVX2 set (Q1): `.cargo/config.toml` `[env]` sets `GGML_AVX`, `GGML_AVX2`, `GGML_FMA` and
  `GGML_F16C` to `ON`, so every local, CI and dist build runs on any AVX2 CPU and never on the build machine's own
  features (`GGML_NATIVE` stays `OFF`, as the build script sets it without `target-cpu=native`). ggml reads them only
  for x86 targets. They travel as env vars, not `rustflags`, because dist sets `RUSTFLAGS` on every build.
- MSVC builds link the static C runtime (Q3): `[env]` sets `LLAMA_STATIC_CRT=1` and
  `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded$<$<CONFIG:Debug>:Debug>` for llama.cpp (its
  `cmake_minimum_required(VERSION 3.14...3.28)` makes CMake policy CMP0091 NEW, under which the runtime comes only
  from that variable and the `/MT` that `LLAMA_STATIC_CRT` puts into the flags is overridden by the default `/MD`:
  the link then fails on `__imp_` CRT symbols), and
  `[target.'cfg(all(windows, target_env = "msvc"))'] rustflags` sets `+crt-static` for Rust, which dist's own
  `RUSTFLAGS` repeat under dist, so `ardana.exe` needs no `VCRUNTIME140.dll`; other targets ignore both.
- The build script registers `rerun-if-env-changed` only for the `GGML_*` variables present at its last run: after
  adding one, run `cargo clean -p llama-cpp-sys-2` (with the profile and `--target` of the build) to reconfigure.
- Do not enable a GPU feature on macOS: llama.cpp turns Metal on by default on Apple targets and the build script only
  switches it off for watchOS. Expose `cuda` and `vulkan` as `ardana-llama` (and `ardana`) cargo features that forward
  to `llama-cpp-2/cuda` and `llama-cpp-2/vulkan`; they pass through unverified (Q16), so never claim they work.

### Backend and logging
- Call `LlamaBackend::init()` exactly once per process and keep the value alive for as long as any model or context
  exists (store it in the runtime behind an `Arc` or a `OnceLock`). A second `init` while one is alive returns
  `BackendAlreadyInitialized`; never `init` per request or per model.
- `LlamaBackend` is `Send + Sync`; share one handle between every loaded model.
- Call `send_logs_to_tracing(LogOptions::default().with_logs_enabled(verbose))` before `LlamaBackend::init()`, so
  llama.cpp's and ggml's stderr chatter never mixes into `ardana run` output. `backend.void_logs()` needs the backend,
  so it runs too late: ggml still prints its Metal device lines (`ggml_metal_device_init: ...`) at the first load.

### Loading a model
- Load with `LlamaModel::load_from_file(&backend, path, &params)` where `params` is `LlamaModelParams`. Keep the
  default `use_mmap = true`, so Ollama blobs and HF cache files are read in place and never copied.
- Map `LoadOptions::gpu_layers` explicitly: `-1` (offload all) leaves `n_gpu_layers` at its default `-1` or passes
  `with_n_gpu_layers(u32::MAX)` (clamped to `i32::MAX`, which llama.cpp treats as every layer); `0` passes
  `with_n_gpu_layers(0)`; any other negative value is an error.
- Treat `--gpu-layers 0` as "no layers on the GPU", not "no GPU at all": llama.cpp documents that the GPU may still
  accelerate some work at `-ngl 0`. The CPU check (R2.7) compares argmaxes, not exact probabilities.
- Turn `LlamaModelLoadError` into an error that names the ref and path (R4.4: an unloadable Ollama blob suggests an
  `hf.co/` source); never unwrap. `ardana-registry`'s `ResolvedModel::load` adds the ref and, for `ollama:` refs, the
  suggestion.
- At `26394b4` a GGUF llama.cpp cannot load comes back as `LlamaModelLoadError::NullResult` ("null result from llama
  cpp"), not an abort: checked on a truncated header, an architecture without its hyperparameter keys, random bytes
  after the magic, absurd tensor and key counts, and Ollama's own `gemma3:12b` blob. With logs silenced the reason
  (`key not found ...`) is not in the error.

> Plan note: `LlamaModelParams::with_n_gpu_layers` takes a `u32` in 0.1.157, so `LoadOptions { gpu_layers: -1 }`
> cannot be passed through as-is; use the mapping above.

### Contexts and threads
- `LlamaModel` is `Send + Sync`; `LlamaContext<'a>` is `!Send` and `!Sync` and borrows its model for `'a`. A context
  can never move between threads nor live in the same struct as the model it borrows.
- Implement `LoadedModel: Send` by owning the model and its single context on one dedicated worker thread and handing
  the caller a `Send` handle (a channel sender plus reply channel); `slot_logits` sends one prompt and waits. Do not
  write `unsafe impl Send` around a context. `ardana-llama`'s `LlamaModelHandle` does this; dropping it closes the
  channel and joins the thread, which frees the context before the model.
- `LlamaModelParams` holds raw pointers and is not `Send` either: build it on the worker thread from plain values.

> Plan note: the Contract `LoadedModel: Send` cannot hold a `LlamaContext` directly because 0.1.157 marks it
> `!Send`; the worker-thread handle above (or a fresh context per call) satisfies the trait.

- Build `LlamaContextParams` with `with_n_ctx(NonZeroU32::new(n_ctx))`, `with_n_batch(n_ctx)` and
  `with_n_ubatch(min(2048, n_ctx))`, as decider's `engine_gguf.py` does, so any prompt that fits the window goes to
  one `decode` call; keep `n_seq_max` at its default of 1 and the other defaults (threads, flash attention auto)
  that decider also keeps. The defaults (n_ctx 512, n_batch 2048, n_ubatch 512) are never right for Ardana.
- `LoadOptions::default()` is decider's GGUF engine default: `n_ctx` 40,960, every layer offloaded.
- Report `LoadedModel::n_ctx()` from `ctx.n_ctx()` after creation, not from the requested value.

### One prompt per decode
- Decode exactly one prompt per call, alone in the context (decider `engine_gguf.py`); never pack several prompts
  into one batch or sequence: packing moved Q4_K_M probabilities by up to 0.16.
- Before each prompt call `ctx.clear_kv_cache()` (it wraps `llama_memory_clear(mem, true)`, clearing metadata and
  data). Never reuse a cached prefix or use `clear_kv_cache_seq` for partial removal; Qwen3.5 is a hybrid model with
  recurrent state, and partial removals may fail (the call returns `false`).
- Fill a `LlamaBatch::new(ids.len(), 1)` with `batch.add(LlamaToken::new(id), pos, &[0], is_slot)` for
  `pos = 0..len`, converting each `u32` id with `i32::try_from` (error, never `as`). Flag `logits = true` only at
  slot positions. `add` returns `BatchAddError` when the batch is full; propagate it.
- Call `ctx.decode(&mut batch)` and map `DecodeError` into `anyhow`; a non-zero return (for example "no KV slot")
  is a runtime error, not a capacity check. `Decider::plan` enforces the context cap before any decode.
- Read each slot with `ctx.get_logits_ith(i)`, where `i` is the batch index of a flagged token. It panics (asserts)
  when `i` was not flagged in the last decode or `i >= n_ctx`, so only pass indices you flagged in this batch.
- Copy the label logits out immediately: the slice borrows the context and is overwritten by the next decode.

### Logits width versus tokenizer vocabulary
- Logit rows are `model.n_vocab()` wide, taken from the GGUF, not from `tokenizer.json`. Qwen3.5 rows are 248,320
  wide while its tokenizer has 248,070 tokens; the padding columns hold meaningless values.
- Index only the label ids passed to `slot_logits`, and never softmax, argmax or sum over a whole row.
- Check at load that every label id is `< n_vocab()` and fail with a clear error otherwise.
- Tokenize with the HF `tokenizer.json` in `ardana-core` (decider does the same); never use `model.str_to_token`
  or the GGUF vocab for prompts.

### Numerics against decider
- Given the same slot logits, Ardana's readout equals decider 1.6.0's (`engine_gguf.py` float32 softmax plus
  `systemone.assemble`) answer for answer, and its prompt ids equal decider's; checked on decider:2b Q4_K_M with
  decider's own code in a sandbox venv, on Metal and at `--gpu-layers 0`.
- The logits themselves depend on the llama.cpp build. decider's `llama-cpp-python` 0.3.35 bundles ggml 0.20 with
  Accelerate BLAS; `llama-cpp-2` 0.1.157 bundles `26394b4` (ggml 0.24) without BLAS. On the ticket fixture the two
  differ by up to 3e-4 in a probability on Metal and up to 1e-2 at `--gpu-layers 0`; argmaxes agree. Compare
  against decider through logits, not through final probabilities, and assert argmaxes and ranges end to end.

## Testing
- Unit tests in `ardana-core` use fake `LoadedModel`s; tests that load a GGUF are `#[ignore]` and run through
  `cargo xtask e2e rust` against the official models in `tmp/hf` (decider:2b Q4_K_M, Qwen3.5-0.8B Q4_0,
  SmolLM3-3B Q4_K_M, Gemma 4 E2B Q4_0) or the local Ollama `llama3.2` blob, never an invented fixture.
- Run the decider:2b ticket check at default offload and at `--gpu-layers 0` and compare argmaxes (R2.7).

## Sources
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/index.html — crate overview, modules, logging redirection
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/llama_backend/struct.LlamaBackend.html — `init` once, `BackendAlreadyInitialized`, `void_logs`, Send/Sync
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/model/struct.LlamaModel.html — `load_from_file`, `new_context`, `n_vocab`, Send/Sync
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/model/params/struct.LlamaModelParams.html — `with_n_gpu_layers(u32)`, `use_mmap`, defaults
- https://docs.rs/llama-cpp-2/0.1.157/src/llama_cpp_2/model/params.rs.html — `u32` to `i32` clamp in `with_n_gpu_layers`
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/context/params/struct.LlamaContextParams.html — `with_n_ctx`, `with_n_batch`, defaults
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/context/struct.LlamaContext.html — `decode`, `get_logits_ith`, `clear_kv_cache`, `!Send`/`!Sync`
- https://docs.rs/llama-cpp-2/0.1.157/src/llama_cpp_2/context.rs.html — `get_logits_ith` assertions on unflagged indices
- https://docs.rs/llama-cpp-2/0.1.157/src/llama_cpp_2/context/kv_cache.rs.html — `clear_kv_cache` wraps `llama_memory_clear`
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/llama_batch/struct.LlamaBatch.html — `new`, `add`, `BatchAddError`
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/token/struct.LlamaToken.html — `LlamaToken(i32)` wrapper
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/fn.send_logs_to_tracing.html — llama.cpp and ggml logs into `tracing`, callable before backend init
- https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/context/params/struct.LlamaContextParams.html#method.with_n_ubatch — `with_n_ubatch`
- https://github.com/utilityai/llama-cpp-rs/blob/0.1.157/llama-cpp-2/Cargo.toml — default features `openmp`, `common`, `android-shared-stdcxx`
- https://github.com/utilityai/llama-cpp-rs — README: build from source, semver policy, cuda feature
- https://github.com/utilityai/llama-cpp-rs/tree/0.1.157/llama-cpp-sys-2 — llama.cpp submodule at `26394b4`
- https://github.com/utilityai/llama-cpp-rs/blob/0.1.157/llama-cpp-sys-2/build.rs — cmake build, `CMAKE_`/`GGML_` env forwarding, cuda/vulkan, watchOS Metal off
- https://github.com/utilityai/llama-cpp-rs/blob/0.1.157/examples/simple/src/main.rs — reference backend/model/batch/decode flow
- https://github.com/ggml-org/llama.cpp/blob/master/docs/build.md — Metal on by default on macOS, `-ngl 0` caveat, CMake flags
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/include/llama.h — batch logits flag, decode return codes, negative `n_gpu_layers`, memory clear
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/src/llama-arch.cpp — `qwen35` architecture name
