# Candle guidelines

Covers Candle, Hugging Face's pure-Rust ML library, as used by `crates/ardana-candle`, the only crate that may depend
on it. `ardana-candle` implements the `ardana-core` traits `Runtime` and `LoadedModel` for safetensors causal LMs
read from a Hugging Face snapshot directory; the `ardana` binary registers it in `Runtimes` after llama.cpp.
Tokenization, prompt building and readout stay in `ardana-core` and never call Candle.

## Versions
- `candle-core` 0.11.0, `candle-nn` 0.11.0, `candle-transformers` 0.11.0 — pinned together with `=0.11.0` in
  `[workspace.dependencies]`: the three crates release in lockstep and `candle-transformers` reads the other two's
  internals. candle-core brings its own `tokenizers` 0.22 (`onig` only, the C library the binary links already),
  which stays in the lock beside ardana's 0.23.2 (Q10).

## Rules

### Build
- Keep the dependency in `ardana-candle` only; `cargo xtask check-deps` lets that crate depend on `ardana-core` alone
  and refuses `ardana-server -> ardana-candle`, like `ardana-llama`.
- No default features, no ONNX Runtime, no C library beyond what the binary links today (Q1): `mkl`, `accelerate`,
  `cuda`, `cudnn`, `nccl`, `flash-attn` and Candle's quantized formats stay off.
- Metal comes only from `ardana-candle/Cargo.toml`'s
  `[target.'cfg(all(target_os = "macos", target_arch = "aarch64"))'.dependencies]`, which turns on `metal` in all three
  crates (Q5); every other target builds the CPU backend alone. Candle's `metal` is optional `objc2-metal` and
  `candle-metal-kernels` dependencies with no build-time shader step. Check with
  `cargo tree -p ardana --target aarch64-apple-darwin -e features -i candle-core` (lists `candle-core feature "metal"`)
  and the same for `x86_64-unknown-linux-gnu` (lists none).
- The dev profile optimises `candle-core`, `candle-nn`, `candle-transformers`, `gemm`, `gemm-common`, `gemm-f32`,
  `gemm-f16` and `half` (`[profile.dev.package.*] opt-level = 3` in the root `Cargo.toml`): unoptimised, one prompt
  of a 1.7B checkpoint takes minutes on the CPU, and `cargo test` builds the real-model tests in that profile.

### Snapshots and architectures
- A safetensors checkpoint is a snapshot directory, `ardana_core::snapshot`: `config.json`, then the shards
  `model.safetensors.index.json#weight_map` names (`snapshot::shards`, each a `.safetensors` file at the repository
  root), else the one `model.safetensors` (`snapshot::weight_files` of a directory). No published crate reads the
  index: the registry's pull reads the listing with `snapshot::shards`, and the runtime, `ResolvedModel::weight_files`
  (`Registry::resolve`, `ardana list`'s size) read a pulled directory with `snapshot::weight_files`.
- The implementation comes from `config.json#architectures` (`snapshot::architectures`), never from a model name or a
  per-model value in code (Q3). `ardana_candle::ARCHITECTURES` lists the names it implements, one implementation each
  (Q2): `Qwen3ForCausalLM` only. `CandleRuntime::supports` is true for a directory whose `config.json` names one of
  them, so `Runtimes::for_weights` hands a GGUF file to llama.cpp and a snapshot directory to Candle.
- Read the architecture's own configuration from the same `config.json` with `serde_json::from_value` into the
  `candle-transformers` type (`models::qwen3::Config`); its unknown fields are ignored.

### Loading
- Map the files with `candle_core::safetensors::MmapedSafetensors::multi(&files)` (an `unsafe` call: the hub cache's
  blobs are never written in place, `huggingface.md`, "Hub cache layout") and load every tensor once into a
  `HashMap<String, Tensor>` on the device in the compute dtype, then build with `VarBuilder::from_tensors`. A tensor
  read twice (a tied head and its embedding) then shares one storage; `VarBuilder::from_mmaped_safetensors` would
  convert it on each `get`. Every tensor is copied out of the mapping before it is dropped.
- Compute (Q5): the CPU in F32 on every target; on an aarch64-apple-darwin build Metal (`Device::new_metal(0)`) in
  BF16 for `gpu_layers` -1 or any count, Candle placing a model whole; `gpu_layers` 0 keeps the CPU; below -1 is an
  error. A failure to open Metal is an error, never a silent CPU fallback.
- `LoadedModel::n_ctx()` is the smaller of `LoadOptions::n_ctx` and `config.json#max_position_embeddings` (Q11): the
  rotary tables end there. The row cap stays the core's (`Limits::context_window`).

### Qwen3 in candle-transformers 0.11.0
- `models::qwen3::Model::forward(input, offset)` returns the final-normed hidden states at every position (`[1, len,
  hidden]`); `ModelForCausalLM` narrows to the last position and applies the whole head, which a readout at several
  slots cannot use. Build `Model` and read the head yourself: `lm_head.weight` (`[vocab, hidden]`), or
  `model.embed_tokens.weight` when `config.json#tie_word_embeddings` is true (Qwen3-1.7B ties).
- `Model::clear_kv_cache` is private in 0.11.0: keep the loaded `Model` unrun and decode each prompt with a clone of
  it (tensors are shared, the key-value cache starts empty), at offset 0. Never reuse a cache between prompts.
- On the CPU the attention is Candle's fused causal kernel and no mask is built; on Metal `forward` builds the causal
  mask itself.
- Off the CPU 0.11.0 falls back to matmul attention over `[heads, len, len]` scores, so Metal memory grows with the
  square of the prompt: Qwen3-1.7B on an M1 Max (release build) peaks at 6.0 GB for one row of 2.2k tokens, 8.9 GB
  for 4.3k and 18.4 GB for 8.7k, in 2.2 s, 6.7 s and 23 s, where its Q8_0 GGUF through llama.cpp stays near 5 GB in
  1.0 s, 2.1 s and 6.0 s. Nothing caps a prompt on Metal below `n_ctx`: for a long state use `--gpu-layers 0` or the
  model's GGUF.
- The CPU is slow on Apple silicon: the same checkpoint in F32 takes 26 s for that 2.2k-token row and 3.5 s for four
  rows of 180 tokens, which Metal answers in 0.3 s. The dev profile is about four times slower again.

### Logits
- Return label logits only: index the hidden states at the slots and the head's rows at the label ids
  (`index_select`), cast both to F32 and multiply, so a prompt never computes a whole vocabulary row. Check, as the
  llama.cpp runtime does, that the prompt is non-empty and within `n_ctx`, every slot inside it and every label id
  below `config.json#vocab_size`.
- The answers equal the GGUF of the same model through llama.cpp up to quantization: Qwen3-1.7B BF16 safetensors
  against `Qwen3-1.7B-Q8_0.gguf` gives the same top choices and noul sides on the ticket and sentiment fixtures, every
  probability within 0.1 (R1.2).

## Testing
- `cargo test -p ardana-candle` reads only Qwen3-1.7B's `config.json` from `tmp/hf` (`cargo xtask fetch --tests`
  installs it, leaving out `.safetensors`); the tests that load the checkpoint are `#[ignore = "e2e: ..."]` and run
  through `cargo xtask e2e rust` against hf.co/Qwen/Qwen3-1.7B at `70d244cc86ccca08cf5af4e1e306ecf908b1ad5e` and its
  GGUF, hf.co/Qwen/Qwen3-1.7B-GGUF `Qwen3-1.7B-Q8_0.gguf` at `90862c4b9d2787eaed51d12237eafdfe7c5f6077` (Q8), never an
  invented fixture.
- `crates/ardana/tests/e2e_registry.rs` pulls the checkpoint online from a loopback stand-in of the Hub
  (`common::HubStandIn`) that records each file asked for, over a hub cache holding links to the fetched blobs, so the
  pull is checked file by file without copying weights.

## Sources
- https://github.com/huggingface/candle — README: pure-Rust ML framework, backends, feature flags
- https://github.com/huggingface/candle/tree/0.11.0 — the 0.11.0 release the pins name
- https://github.com/huggingface/candle/blob/0.11.0/candle-core/Cargo.toml — `metal`, `cuda`, `mkl`, `accelerate` features; `tokenizers` 0.22 with `onig`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/qwen3.rs — `Config`, `Model::forward`, private `embed_tokens` and `clear_kv_cache`, tied head
- https://docs.rs/candle-core/0.11.0/candle_core/enum.Device.html — `Device::Cpu`, `new_metal`, `is_metal`
- https://docs.rs/candle-core/0.11.0/candle_core/safetensors/struct.MmapedSafetensors.html — `multi`, `tensors`, `load`
- https://docs.rs/candle-nn/0.11.0/candle_nn/var_builder/type.VarBuilder.html — `from_tensors`, `from_mmaped_safetensors`, `get`
- https://docs.rs/candle-transformers/0.11.0/candle_transformers/models/qwen3/index.html — the Qwen3 module
- https://huggingface.co/docs/safetensors/index — the safetensors format
- https://huggingface.co/docs/safetensors/metadata_parsing — sharded checkpoints and `model.safetensors.index.json`
- https://huggingface.co/docs/transformers/main_classes/configuration — `config.json`, `architectures`, `tie_word_embeddings`, `max_position_embeddings`
