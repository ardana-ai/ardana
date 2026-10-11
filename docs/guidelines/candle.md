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
  (Q2): `Qwen3ForCausalLM`, `Qwen2ForCausalLM`, `SmolLM3ForCausalLM`, `LlamaForCausalLM` and `Phi3ForCausalLM`.
  `CandleRuntime::supports` is true for a directory whose `config.json` names one of them, so
  `Runtimes::for_weights` hands a GGUF file to llama.cpp and a snapshot directory to Candle.
- One architecture is four things in `crates/ardana-candle/src/lib.rs` and nothing else: its name in `ARCHITECTURES`,
  its arm in `configuration`, `impl Architecture` for its configuration type (`shape`: the vocabulary, the hidden
  size, the positions and whether the head is tied; `build`: the decoder from the root `VarBuilder`) and
  `impl Decoder` for its model (`hidden_states`: the final-normed states at every position of one prompt). The
  readout is `CausalLm`'s, shared: no architecture reads a head, checks a prompt or multiplies label rows itself.
  An architecture whose candle-transformers model cannot return those states adds its vendored module file
  ("Vendored modules").
- Read the architecture's own configuration from the same `config.json` with `serde_json::from_value` into the
  `candle-transformers` type (`models::qwen3::Config`, `models::qwen2::Config`, `models::smol::smollm3::Config`,
  `models::llama::LlamaConfig`, `models::phi3::Config`); its unknown fields are ignored, and it is read before any
  tensor, so a configuration that does not read costs no load. `ardana-candle` depends on neither `serde` nor
  `tracing`: a field the candle type lacks is read from the same value beside it (`llama_config` reads `head_dim`).

### Vendored modules
- Where a candle-transformers 0.11.0 model narrows to the last position inside the model and keeps its layers
  private, copy that one module file to `crates/ardana-candle/src/<module>.rs` (Q4) and change only the narrowing
  and what a real checkpoint of the architecture needs. The file opens with where it comes from (the upstream path
  and `candle-transformers 0.11.0`), candle's Apache-2.0 notice (candle is MIT OR Apache-2.0 and names no copyright
  holder) and the list of every change, which Apache-2.0 asks of a changed file. Everything else stays as upstream
  wrote it, formatted by `cargo fmt`.
- A vendored file passes clippy like any other and silences no lint: what upstream has and nothing here calls
  (constructors for other checkpoints, flash attention, `clear_kv_cache`, a file's own tracing spans) is left out
  and listed. It imports what candle-transformers exports instead of copying it: the configuration types that
  derive `Deserialize`, `models::with_tracing::{Linear, RmsNorm}` and `utils::{repeat_kv, build_causal_mask}`.
- A missing tensor is an error of the load, never a panic: replace an `unwrap()` on the load path with `?`.
- The modules today, each with the change it is vendored for:
  - `llama.rs` (`src/models/llama.rs`, `LlamaForCausalLM`): `Llama::forward` returns the final-normed hidden states
    at every position instead of the last position's logits, and `Llama` holds no `lm_head`. Also `Config::head_dim`
    from `config.json#head_dim`, and a layer's missing tensor as an error where upstream panics.
  - `phi3.rs` (`src/models/phi3.rs`, `Phi3ForCausalLM`): `Model::forward` returns the final-normed hidden states at
    every position instead of the last position's logits, and `Model` holds no `lm_head`. Also `longrope` scaled as
    transformers scales it.

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

### Qwen2 and SmolLM3 in candle-transformers 0.11.0
- `Qwen2ForCausalLM`: `models::qwen2::Model::forward(input, 0, None)` returns the final-normed hidden states at every
  position; `ModelForCausalLM` narrows to the last one. Pass no attention mask: `None` builds the causal mask, while
  `Some(mask)` builds a padding mask that is not causal. The head is `model.embed_tokens.weight` when
  `tie_word_embeddings` is true (Qwen2.5-1.5B-Instruct ties), else `lm_head.weight`.
- `qwen2::Config` wants all 14 fields with `sliding_window` a number (a `null` does not read), and `forward` masks
  every layer by that window whatever `use_sliding_window` says. `qwen2_config` therefore reads a checkpoint with
  `use_sliding_window: false` (Qwen2.5's) with a window as wide as `max_position_embeddings`, which masks nothing, and
  refuses `use_sliding_window: true`: transformers windows only the layers from `max_window_layers` on.
- `SmolLM3ForCausalLM`: `models::smol::smollm3::Model::forward(input, 0)` returns the final-normed hidden states at
  every position, masking causally itself. A layer skips its rotary embedding where `no_rope_layers` holds a 0 (else
  by `no_rope_layer_interval`); `use_sliding_window: true` makes `Model::new` fail; SmolLM3-3B ties its head.
- Both models have a public `clear_kv_cache`, and both still decode each prompt with a clone, as Qwen3 does: one rule
  for every architecture, and the clone's cache goes with the call (SmolLM3's `KvCache` keeps the buffer a long
  prompt grew it to after a reset).
- Both use matmul attention over `[heads, len, len]` scores on every device, the CPU too (Qwen3 alone has the fused
  CPU kernel), so memory grows with the square of the prompt: Qwen2.5-1.5B-Instruct in F32 on a 16-core x86 CPU
  answers the ticket's two rows over a 5.8k-token state in 271 s at an 11.6 GB peak, where its Q8_0 GGUF through
  llama.cpp takes 145 s and 3.4 GB. SmolLM3-3B in F32 holds 18 GB for a 140-token request (its GGUF 6.4 GB).
- Not measured on Metal (no Apple machine, 2026-10-11): both keep the BF16 rule. `qwen2` computes its rotary table in
  the model's dtype, where `qwen3` and `smollm3` compute it in F32 and convert, so in BF16 its positions past 256 are
  not exact; measure a long prompt against the GGUF before relying on Qwen2 on Metal.

### Llama and Phi-3 in the vendored modules
- `LlamaForCausalLM`: `llama::Llama::forward(input, 0, &mut cache)` returns the final-normed hidden states at every
  position. Llama keeps its rotary tables in the `Cache` it is handed, so `build` computes one at load
  (`use_kv_cache: false`) and each prompt runs on a clone of it, which stores no keys or values and drops its mask
  with the call. `rope_scaling` reads `rope_type` `llama3` (Llama 3.x) or `default`, or `null`.
- `head_dim` is `config.json#head_dim` when a checkpoint writes it out, else `hidden_size / num_attention_heads`:
  MiniCPM5-1B has 16 heads of 128 over 1536 hidden, so its `q_proj` is 2048 wide. `LlamaConfig` reads an absent
  `tie_word_embeddings` as false: Llama-3.2-1B ties its head, MiniCPM5-1B has its own `lm_head.weight`.
- `Phi3ForCausalLM`: `phi3::Model::forward(input, 0)` returns the final-normed hidden states at every position, on a
  clone like every other architecture. `qkv_proj` and `gate_up_proj` are fused tensors, and a `partial_rotary_factor`
  (Phi-4-mini: 0.75) rotates only the first dimensions of a head.
- `longrope` (`rope_scaling.type`, Phi-4-mini) follows transformers: a rotary frequency is divided by its factor; a
  prompt longer than `original_max_position_embeddings` (4096) reads `long_factor` at every position and any other
  prompt `short_factor`; sin and cos are multiplied by `sqrt(1 + ln(max / original) / ln(original))` (1.19 for
  131,072 over 4,096) at every length. candle-transformers 0.11.0 multiplies where it should divide, switches
  factors by position and has no attention factor. llama.cpp picks the factors by its context size, not by the
  prompt: at ardana's default window of 40,960 a Phi-4-mini GGUF reads the long factors for a 70-token prompt, which
  Candle and transformers read with the short ones. Its answers to the fixtures change between `--n-ctx 4096` and
  `--n-ctx 4097`, and at 4096 they are Candle's within 0.0001.
- Checked against transformers 5.19.0 in F32 on the CPU (2026-10-11, by hand): the label logits of Llama-3.2-1B,
  MiniCPM5-1B and Phi-4-mini through Candle are within 0.0001 of it at 29 and at 4.4k tokens, so what separates a
  checkpoint from its GGUF is the quantization and, for Phi-4-mini below 4,097 tokens, llama.cpp's factors.
- A `Phi3ForCausalLM` checkpoint without `rope_scaling` (microsoft/phi-4, which also has its own `lm_head.weight`)
  takes upstream's unscaled table unchanged. It was not run: its 29 GB of BF16 safetensors are 58 GB in F32.
- Both compute attention by matmul over `[heads, len, len]` scores on every device (Llama casts them to F32 and
  masks with a u8 mask, Phi-3 adds a mask in the model's dtype), so memory and time grow with the square of the
  prompt. In F32 on a 16-core x86 CPU a load and one row of 4.4k tokens take Llama-3.2-1B 144 s at a 12.5 GB peak,
  MiniCPM5-1B 115 s at 8.3 GB and Phi-4-mini 207 s at 22.5 GB; Phi-4-mini holds 22.6 GB for a 70-token request, its
  7.7 GB of BF16 weights read as F32 (its Q8_0 GGUF: 9.4 GB at the 40,960 window), and answers the ticket's two
  rows over a 4.9k-token state in 457 s, where the GGUF through llama.cpp takes 291 s.
- Not measured on Metal (no Apple machine, 2026-10-11): both keep the BF16 rule. Llama computes its rotary table and
  its attention in F32. Phi-3 computes the `longrope` tables in F32 and converts them, but the unscaled table (phi-4)
  in the model's dtype as upstream does, so in BF16 its positions past 256 are not exact, and its attention runs in
  the model's dtype; measure a long prompt against the GGUF before relying on Phi-3 on Metal.

### Logits
- Return label logits only: index the hidden states at the slots and the head's rows at the label ids
  (`index_select`), cast both to F32 and multiply, so a prompt never computes a whole vocabulary row. Check, as the
  llama.cpp runtime does, that the prompt is non-empty and within `n_ctx`, every slot inside it and every label id
  below `config.json#vocab_size`.
- The answers equal the GGUF of the same model through llama.cpp up to quantization: Qwen3-1.7B BF16 safetensors
  against `Qwen3-1.7B-Q8_0.gguf` gives the same top choices and noul sides on the ticket and sentiment fixtures, every
  probability within 0.1 (R1.2). `cargo xtask e2e candle` holds every other architecture to the same on the ticket,
  sentiment and score fixtures, in F32 on the CPU against the Q8_0 GGUF: Qwen2.5-1.5B-Instruct stays within 0.001
  (0.008 over a 5.4k-token state), SmolLM3-3B within 0.003, Llama-3.2-1B within 0.014, MiniCPM5-1B within 0.037 (a
  noul near 0.6) and Phi-4-mini within 0.001 (0.0002 over a 4.9k-token state).

## Testing
- `cargo test -p ardana-candle` reads only Qwen3-1.7B's `config.json` from `tmp/hf` (`cargo xtask fetch --tests`
  installs it, leaving out `.safetensors`); the tests that load the checkpoint are `#[ignore = "e2e: ..."]` and run
  through `cargo xtask e2e rust` against hf.co/Qwen/Qwen3-1.7B at `70d244cc86ccca08cf5af4e1e306ecf908b1ad5e` and its
  GGUF, hf.co/Qwen/Qwen3-1.7B-GGUF `Qwen3-1.7B-Q8_0.gguf` at `90862c4b9d2787eaed51d12237eafdfe7c5f6077` (Q8), never an
  invented fixture.
- A change a vendored module carries gets a plain unit test over random weights (`candle_nn::VarMap`, a model a few
  values wide built through `configuration` and `Architecture::build`), which pins the change and nothing about a
  real model's answers: "llama_reads_a_written_head_dim" (the projections' shapes, a state at every position, each
  the one the prompt cut there has, a missing tensor as an error) and "phi3_scales_longrope_by_the_prompt_length"
  (the rotary table read back through `apply_rotary_emb_qkv`). The real checkpoints stay the suite's.
- `crates/ardana/tests/e2e_registry.rs` pulls the checkpoint online from a loopback stand-in of the Hub
  (`common::HubStandIn`) that records each file asked for, over a hub cache holding links to the fetched blobs, so the
  pull is checked file by file without copying weights.
- Every other architecture is verified by `cargo xtask e2e candle` (`xtask/src/candle.rs`) on one row of
  `xtask/src/e2e.rs#CANDLE_FIXTURES`: a checkpoint and the GGUF of the same model, each pinned to a commit. `cargo
  xtask fetch` installs neither; the suite pulls a row from the Hub, checks it and removes its files before the next
  (the disk does not hold them all), so these checkpoints have no `#[ignore]`d test. A row passes when the pull
  records runtime `candle` at the pinned commit, `ardana run --json` answers `ticket.json`, `sentiment.json` and
  `score.json` like the GGUF (the same choices and noul sides, every probability within 0.1), and one `ardana serve`
  answers the three and then the first again with identical probabilities; a `long_state` row also answers the
  ticket's state repeated past 4,500 tokens through both runtimes. A new architecture adds its row there: today
  Qwen/Qwen2.5-1.5B-Instruct at `989aa7980e4cf806f80c7fef2b1adb7bc71aa306` against Qwen/Qwen2.5-1.5B-Instruct-GGUF
  `qwen2.5-1.5b-instruct-q8_0.gguf` at `91cad51170dc346986eccefdc2dd33a9da36ead9` (`long_state`),
  HuggingFaceTB/SmolLM3-3B at `a07cc9a04f16550a088caea529712d1d335b0ac1` against ggml-org/SmolLM3-3B-GGUF
  `SmolLM3-Q8_0.gguf` at `4965cb60b150737b68a0408c36aeefb65078f894`, unsloth/Llama-3.2-1B-Instruct at
  `5a8abab4a5d6f164389b1079fb721cfab8d7126c` against bartowski/Llama-3.2-1B-Instruct-GGUF
  `Llama-3.2-1B-Instruct-Q8_0.gguf` at `067b946cf014b7c697f3654f621d577a3e3afd1c`, openbmb/MiniCPM5-1B at
  `87179e5c1f455ef22e6223592d2d61351b525bfc` against openbmb/MiniCPM5-1B-GGUF `MiniCPM5-1B-Q8_0.gguf` at
  `3d55fac80935ae6456986ad2384b5cbcc4d6c948`, and microsoft/Phi-4-mini-instruct at
  `cfbefacb99257ffa30c83adab238a50856ac3083` against unsloth/Phi-4-mini-instruct-GGUF
  `Phi-4-mini-instruct.Q8_0.gguf` at `78eb92a46fc37e6b524df991ed9aca9bc6aa7b80` (`long_state`).

## Sources
- https://github.com/huggingface/candle — README: pure-Rust ML framework, backends, feature flags
- https://github.com/huggingface/candle/tree/0.11.0 — the 0.11.0 release the pins name
- https://github.com/huggingface/candle/blob/0.11.0/candle-core/Cargo.toml — `metal`, `cuda`, `mkl`, `accelerate` features; `tokenizers` 0.22 with `onig`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/qwen3.rs — `Config`, `Model::forward`, private `embed_tokens` and `clear_kv_cache`, tied head
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/qwen2.rs — `Config`, `Model::forward` and its masks, the sliding window, the rotary table's dtype
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/smol/smollm3.rs — `Config::should_skip_rope`, `Model::forward`, `KvCache`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/llama.rs — the file `llama.rs` is copied from: `LlamaConfig`, `Cache`, `Llama::forward` and its last-position narrowing, the `unwrap()` in `Llama::load`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/phi3.rs — the file `phi3.rs` is copied from: `Config`, `RotaryEmbedding::new` and its `longrope` branch, `Model::forward`
- https://github.com/huggingface/candle/blob/0.11.0/LICENSE-APACHE — the license the vendored files are taken under
- https://huggingface.co/docs/transformers/internal/rope_utils — `longrope` and `llama3` rope scaling: `short_factor`, `long_factor`, `original_max_position_embeddings`, `attention_factor`
- https://huggingface.co/docs/transformers/model_doc/phi3 — `Phi3Config`: `rope_parameters` (a `config.json`'s `rope_scaling`), `original_max_position_embeddings`
- https://huggingface.co/docs/transformers/model_doc/llama — `LlamaConfig`: `head_dim`, `rope_parameters` (a `config.json`'s `rope_scaling`), `tie_word_embeddings`
- https://docs.rs/candle-core/0.11.0/candle_core/enum.Device.html — `Device::Cpu`, `new_metal`, `is_metal`
- https://docs.rs/candle-core/0.11.0/candle_core/safetensors/struct.MmapedSafetensors.html — `multi`, `tensors`, `load`
- https://docs.rs/candle-nn/0.11.0/candle_nn/var_builder/type.VarBuilder.html — `from_tensors`, `from_mmaped_safetensors`, `get`
- https://docs.rs/candle-transformers/0.11.0/candle_transformers/models/qwen3/index.html — the Qwen3 module
- https://huggingface.co/docs/safetensors/index — the safetensors format
- https://huggingface.co/docs/safetensors/metadata_parsing — sharded checkpoints and `model.safetensors.index.json`
- https://huggingface.co/docs/transformers/main_classes/configuration — `config.json`, `architectures`, `tie_word_embeddings`, `max_position_embeddings`
