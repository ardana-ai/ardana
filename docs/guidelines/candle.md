# Candle guidelines

Covers Candle, Hugging Face's pure-Rust ML library, as used by `crates/ardana-candle`, the only crate that may depend
on it. `ardana-candle` implements the `ardana-core` traits `Runtime` and `LoadedModel` for safetensors causal LMs
read from a Hugging Face snapshot directory, and `DecisionModel` for the THX-01 decision model ("THX-01, the
decision model"); the `ardana` binary registers it in `Runtimes` after llama.cpp. Tokenization, prompt and sequence
building and readout stay in `ardana-core` and never call Candle.

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
  (Q2): `Qwen3ForCausalLM`, `Qwen2ForCausalLM`, `SmolLM3ForCausalLM`, `LlamaForCausalLM`, `Phi3ForCausalLM`,
  `GraniteForCausalLM`, `Lfm2ForCausalLM`, `Gemma3ForCausalLM`, `Gemma3ForConditionalGeneration` (its text tower),
  `Olmo3ForCausalLM`, and `THXDecisionModel`, the one decision model.
  `CandleRuntime::supports` is true for a directory whose `config.json`
  names one of them, so `Runtimes::for_weights` hands a GGUF file to llama.cpp and a snapshot directory to Candle.
  `CandleRuntime::load` returns a `Loaded::Causal` for a causal LM and a `Loaded::Decision` for `THXDecisionModel`.
- `ardana finetune --format safetensors` writes such a directory, `$ARDANA_HOME/models/<name>/`, for the causal names
  of `ARCHITECTURES` (every name but `THXDecisionModel`, read from the list): the merged safetensors beside the
  trained checkpoint's own `config.json`. A `config.json` transformers 5 saves does not read here: it writes
  `rope_parameters` without `rope_theta`, which `qwen3::Config` requires (`missing field rope_theta`).
- One architecture is four things in `crates/ardana-candle/src/lib.rs` and nothing else: its name in `ARCHITECTURES`,
  its arm in `configuration`, `impl Architecture` for its configuration type (`shape`: the vocabulary, the hidden
  size, the positions and whether the head is tied; `build`: the decoder from the root `VarBuilder`) and
  `impl Decoder` for its model (`hidden_states`: the final-normed states at every position of one prompt). The
  readout is `CausalLm`'s, shared: no architecture reads a head, checks a prompt or multiplies label rows itself.
  What an architecture asks of the readout it says through `Architecture`, never in a readout of its own:
  `logits_scaling` (1 unless overridden; Granite's) is what `CausalLm` divides the label logits by, and
  `final_logit_softcapping` (none unless overridden; Gemma 3's) what it then softcaps them with, as
  `cap * tanh(logits / cap)`, elementwise, so the label rows alone softcap as a whole vocabulary row would. What
  an architecture reads of a checkpoint it says through `Architecture::tensor`: every tensor under its own name,
  unless overridden (the text tower of a multimodal checkpoint, which strips its prefix and skips the rest).
  An architecture whose candle-transformers model cannot return those states adds its vendored module file
  ("Vendored modules").
- Read the architecture's own configuration from the same `config.json` with `serde_json::from_value` into the
  `candle-transformers` type (`models::qwen3::Config`, `models::qwen2::Config`, `models::smol::smollm3::Config`,
  `models::llama::LlamaConfig`, `models::phi3::Config`, `models::granite::GraniteConfig`,
  `models::lfm2::Lfm2Config`, `models::gemma3::Config`, `models::olmo2::Config`); its unknown fields are ignored, and
  it is read before any
  tensor, so a configuration
  that does not read costs no load. `ardana-candle` depends on neither `serde` nor `tracing`: a field the candle
  type lacks is read from the same value beside it (`llama_config` reads `head_dim`, `granite_config` the four
  scalars and `tie_word_embeddings`, `lfm2_config` `intermediate_size`, `block_auto_adjust_ff_dim` and the tying,
  `gemma3_config` the linear `rope_scaling`'s factor, `olmo3_config` `layer_types`, `sliding_window` and the yarn
  `rope_scaling`). A multimodal checkpoint's text configuration is nested:
  `Gemma3ForConditionalGeneration` reads `config.json#text_config` as `Gemma3ForCausalLM` reads the root.

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
  - `qwen2.rs` (`src/models/qwen2.rs`, `Qwen2ForCausalLM`): F32 rotary products and trig before conversion to the
    model dtype, shared query-chunk attention and no key-value cache. It keeps upstream's all-position hidden
    states and imports its `Config`; unused padding masks, incremental decode and `ModelForCausalLM` are left out.
  - `llama.rs` (`src/models/llama.rs`, `LlamaForCausalLM`): `Llama::forward` returns the final-normed hidden states
    at every position instead of the last position's logits, and `Llama` holds no `lm_head`. Also `Config::head_dim`
    from `config.json#head_dim`, and a layer's missing tensor as an error where upstream panics.
  - `phi3.rs` (`src/models/phi3.rs`, `Phi3ForCausalLM`): `Model::forward` returns the final-normed hidden states at
    every position instead of the last position's logits, and `Model` holds no `lm_head`. Also `longrope` scaled as
    transformers scales it, F32 rotary products and trig before conversion, shared query-chunk attention and no
    key-value cache.
  - `granite.rs` (`src/models/granite.rs`, `GraniteForCausalLM`): `Granite::forward` returns the final-normed hidden
    states at every position instead of the last position's logits, and `Granite` holds no `lm_head`. Also
    `Config::multipliers`, the four scalars of a Granite `config.json` that upstream's `granite` does not read
    (`embedding_multiplier`, `attention_multiplier`, `residual_multiplier`, `logits_scaling`),
    `Config::tie_word_embeddings`, and a layer's missing tensor as an error where upstream panics.
  - `lfm2.rs` (`src/models/lfm2.rs`, `Lfm2ForCausalLM`): `Model::forward` returns the final-normed hidden states at
    every position instead of the last position's logits, and `Model` holds no `lm_head`. Also the feed-forward
    width as transformers computes it from `intermediate_size`, and the tying handed in by the caller.
  - `gemma3.rs` (`src/models/gemma3.rs`, `Gemma3ForCausalLM` and the text tower of
    `Gemma3ForConditionalGeneration`): `Model::forward` returns the final-normed hidden states at every position
    instead of the last position's softcapped logits, and `Model` holds neither `lm_head` nor
    `final_logit_softcapping`. Also the sliding mask's window, the attention scale and the global layers' linear
    rope scaling as transformers and llama.cpp have them ("Gemma 3 in the vendored module"), F32 rotary products
    and trig before conversion, shared query-chunk attention and no key-value cache.
  - `olmo2.rs` (`src/models/olmo2.rs`, `Olmo3ForCausalLM`): `Model::forward` returns the final-normed hidden states
    at every position instead of the last position's logits, and `Model` holds no `lm_head`. Also Olmo 3's
    `layer_types` sliding layers under their window's mask and the full layers' yarn rope scaling, the attention
    over chunks of query rows and no key-value cache, which a 7B model's memory asks for ("Olmo 3 in the vendored
    module"), and F32 rotary products and trig before conversion.

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

### Whole-prompt rotary and attention
- Qwen2, Phi-3, Gemma 3 and Olmo 3 share `ops::rotary`: positions, inverse frequencies, their products, sin/cos and
  the attention factor are computed in F32; only the finished tables are converted to the model dtype. Converting
  positions to BF16 first merges adjacent positions past 256 and corrupts long-prompt rotations. Model weights,
  hidden states and attention remain BF16 on native Metal; the CPU remains F32.
- These four decoders share `ops::attention`, covering at most 512 query rows per score tensor while retaining all
  keys. The operation order stays scale, optional attention softcap, causal/sliding mask, softmax, value product;
  chunks concatenate along the query dimension. Work still grows quadratically with prompt length, but individual
  score tensors have shape `[batch, heads, min(512, len), len]`.
- On Metal each chunk synchronizes after its score temporaries drop. candle-core 0.11.0's `Device::synchronize`
  waits for the queued command buffers, releases them, clears their fence-output references and removes unused
  buffers from its pools and Metal residency set. Chunking without this completion boundary allows asynchronous
  layers to retain their scores until the final readback. CPU chunks need no synchronization.
- These decoders store no key-value cache: the runtime decodes each prompt whole at offset 0. In particular Phi
  does not retain every layer's keys/values, and Gemma does not allocate global caches to `max_position_embeddings`
  for a single prompt. This changes no context limit, device selection or readout.

### THX-01, the decision model
- `THXDecisionModel` (`hf.co/doofz/THX-01` at `bef2e2478be7acd79429324a658558a57552a2e8`, `config.json` with
  `model_type` `thx01`) is the `thx01` 1.0.0 package's `DecisionModel` (`thx01/common.py`): mmBERT-base, a 22-layer
  ModernBERT encoder (`encoder/config.json`, `ModernBertForMaskedLM`, hidden 768, 12 heads, `intermediate_size` 1152,
  vocabulary 256,000, `global_attn_every_n_layers` 3, `local_attention` 128), the question type's row of
  `type_emb.weight` (`[3, 768]`: choice 0, score 1, noul 2) added to every position, two PyTorch
  `TransformerEncoderLayer(768, 12 heads, 3072, norm_first=True)` (ReLU, every bias on, `head.layers.{0,1}` with
  the packed `self_attn.in_proj_weight` `[2304, 768]` and `in_proj_bias`, `self_attn.out_proj`, `linear1`,
  `linear2`, `norm1`, `norm2`) and the scorer (`scorer.0` LayerNorm, `scorer.1` Linear 768→768, GELU, `scorer.3`
  Linear 768→1) read at the sequence's markers: one logit per option. The checkpoint's 170 tensors are F16;
  `act_head.*` (the action head, no answer) and `temperature` (a buffer the package never applies) are never read.
  It lives in `crates/ardana-candle/src/thx01.rs`, not in `CausalLm`: it has no head, no slots and no label logits.
- The encoder is candle-transformers 0.11.0's `models::modernbert::ModernBert`, unvendored: `ModernBert::load(vb,
  &Config)` reads `model.embeddings.tok_embeddings`, `model.embeddings.norm`, `model.layers.{i}` (`attn.Wqkv`,
  `attn.Wo`, `mlp.Wi`, `mlp.Wo`, `attn_norm` where a layer has one, `mlp_norm`) and `model.final_norm` from the root
  `VarBuilder`, so `thx01::tensor` reads the checkpoint's `encoder.*` under `model.*` (through `read_tensors`, which
  takes the naming as a closure) and the head's, the scorer's and `type_emb.weight` under their own names;
  `forward(ids [1, len], mask [1, len])` returns the final-normed states at every position, bidirectional and
  stateless, with layer `i` local when `i % global_attn_every_n_layers != 0` and masked to `|i - j| <=
  local_attention / 2`, as transformers masks it. A sequence runs alone (batch 1, a mask of ones): the package pads
  a request's questions into one batch and masks the padding, which gives the same numbers.
- `thx01::encoder_config` builds `modernbert::Config` from `encoder/config.json` by hand: candle's `Config` wants
  `global_rope_theta` and `local_rope_theta`, which mmBERT writes as `rope_parameters.full_attention.rope_theta` and
  `rope_parameters.sliding_attention.rope_theta` (160,000 both; the flat names are read when the nested ones are
  absent), and `layer_norm_eps`, read from `norm_eps` (1e-5), else `layer_norm_eps`. candle's layers have no biases
  and alternate by the count alone, so `norm_bias`, `attention_bias` or `mlp_bias` on, or `layer_types` disagreeing
  with `global_attn_every_n_layers`, is refused at load. `thx01::Config::new` reads `rl_agent_config.json` beside it:
  `head_layers` (2) and the temperatures the model applies, `temperature_by_options["<type>:<bucket>"]` (buckets `2`,
  `3-5`, `6-10`, `11+` by option count: `noul:2` 2.0, `choice:3-5` 2.55, `choice:6-10` 2.3, `score:3-5` 3.05,
  `choice:11+` 2.55) else `temperature[<type>]` (2.55, 3.05, 2.0), each clamped to [0.5, 5.0] as the package clamps
  it (`ardana_core::profile::clamp_temperature`). The engine's `softmax(logits, T)` over the marker logits is the
  probability vector `DecisionModel::decide` returns; the engine assembles it as it assembles a causal model's.
- Compute (Q5): F32 on every device (`thx01::DTYPE`). `read_tensors` converts the F16 tensors once; on an Apple
  silicon build `gpu_layers` -1 places them on Metal in F32, never BF16, since candle's ModernBERT adds F32 masks to
  the scores and the head is written for F32. `gpu_layers` 0 keeps the CPU, below -1 is an error, as for every
  architecture; `LoadOptions::n_ctx` means nothing to it (the window is the profile's `decision.window`, 1,024, checked
  by `Decider::plan` before the load; a sequence past `max_position_embeddings`, 8192, is a load-time error too).
  Not measured on Metal (no Apple machine, 2026-10-12).
- The sequence format, the head budget and the option cuts are the core's (`ardana_core::thx01`, one sequence per
  question: `[CLS] <type> question: <instructions> [SEP] [MASK] <option> … [SEP] <state> [SEP]`, `<bos>`, `<eos>` and
  `<mask>` of the Gemma tokenizer in `tokenizer/`); the state is never cut: a sequence past the window is refused
  (Q9) where the package truncates, so the two agree token for token inside the window. `cargo xtask e2e candle`
  holds the row `thx-01` to the `thx01` package's own answers on the ticket, sentiment and score fixtures
  (`crates/ardana-core/tests/data/thx01/`, `cargo xtask export-thx01`): the golden's choices, every probability
  within 0.01 and the golden's `input_tokens` (89, 72 and 115).

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

### Qwen2 in the vendored module and SmolLM3 in candle-transformers 0.11.0
- `Qwen2ForCausalLM`: `qwen2::Model::forward(input, 0)` returns the final-normed hidden states at every
  position; upstream's `ModelForCausalLM` narrows to the last one. The decoder builds the causal mask. The head is
  `model.embed_tokens.weight` when
  `tie_word_embeddings` is true (Qwen2.5-1.5B-Instruct ties), else `lm_head.weight`.
- `qwen2::Config` wants all 14 fields with `sliding_window` a number (a `null` does not read), and `forward` masks
  every layer by that window whatever `use_sliding_window` says. `qwen2_config` therefore reads a checkpoint with
  `use_sliding_window: false` (Qwen2.5's) with a window as wide as `max_position_embeddings`, which masks nothing, and
  refuses `use_sliding_window: true`: transformers windows only the layers from `max_window_layers` on.
- `SmolLM3ForCausalLM`: `models::smol::smollm3::Model::forward(input, 0)` returns the final-normed hidden states at
  every position, masking causally itself. A layer skips its rotary embedding where `no_rope_layers` holds a 0 (else
  by `no_rope_layer_interval`); `use_sliding_window: true` makes `Model::new` fail; SmolLM3-3B ties its head.
- SmolLM3 decodes each prompt with a clone, as Qwen3 does, so its `KvCache` and any buffers a long prompt grew go
  with the call. Qwen2 stores no cache and runs the loaded decoder directly.
- SmolLM3 uses matmul attention over `[heads, len, len]` scores on every device (Qwen3 alone has the fused CPU
  kernel); Qwen2 now uses the shared query chunks. Earlier unchunked F32 CPU measurements had Qwen2.5-1.5B-Instruct
  answer two rows over a 5.8k-token state in 271 s at an 11.6 GB peak (its Q8_0 GGUF: 145 s, 3.4 GB).
  SmolLM3-3B in F32 holds 18 GB for a 140-token request (its GGUF 6.4 GB).
- Qwen2's rotary table follows the F32-before-conversion rule above, as upstream Qwen3 and SmolLM3 already do.

### Llama and Phi-3 in the vendored modules
- `LlamaForCausalLM`: `llama::Llama::forward(input, 0, &mut cache)` returns the final-normed hidden states at every
  position. Llama keeps its rotary tables in the `Cache` it is handed, so `build` computes one at load
  (`use_kv_cache: false`) and each prompt runs on a clone of it, which stores no keys or values and drops its mask
  with the call. `rope_scaling` reads `rope_type` `llama3` (Llama 3.x) or `default`, or `null`.
- `head_dim` is `config.json#head_dim` when a checkpoint writes it out, else `hidden_size / num_attention_heads`:
  MiniCPM5-1B has 16 heads of 128 over 1536 hidden, so its `q_proj` is 2048 wide. `LlamaConfig` reads an absent
  `tie_word_embeddings` as false: Llama-3.2-1B ties its head, MiniCPM5-1B has its own `lm_head.weight`.
- `Phi3ForCausalLM`: `phi3::Model::forward(input, 0)` returns the final-normed hidden states at every position, on a
  stateless decoder with no key-value cache. `qkv_proj` and `gate_up_proj` are fused tensors, and
  `partial_rotary_factor`
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
  takes the unscaled table, computed in F32 before conversion. It was not run: its 29 GB of BF16 safetensors are 58 GB in F32.
- Llama computes attention by matmul over `[heads, len, len]` scores, cast to F32 and masked with a u8 mask; Phi-3
  uses the shared query chunks and adds a mask in the model's dtype. Earlier unchunked F32 measurements on a
  16-core x86 CPU had a load and one row of 4.4k tokens take Llama-3.2-1B 144 s at a 12.5 GB peak,
  MiniCPM5-1B 115 s at 8.3 GB and Phi-4-mini 207 s at 22.5 GB; Phi-4-mini holds 22.6 GB for a 70-token request, its
  7.7 GB of BF16 weights read as F32 (its Q8_0 GGUF: 9.4 GB at the 40,960 window), and answers the ticket's two
  rows over a 4.9k-token state in 457 s, where the GGUF through llama.cpp takes 291 s.
- Llama computes its rotary table and attention in F32. Phi-3 computes both its `longrope` and unscaled tables
  in F32 before conversion; its attention remains in the model's dtype and completes each Metal query chunk.

### Granite and LFM2 in the vendored modules
- `GraniteForCausalLM`: `granite::Granite::forward(input, 0, &mut cache)` returns the final-normed hidden states at
  every position. Like Llama it keeps its rotary tables in the `Cache` it is handed: `build` computes one at load
  (`use_kv_cache: false`) and each prompt runs on a clone. `rope_scaling` reads `rope_type` `granite` or `default`,
  or `null` (Granite 4.2).
- A Granite `config.json` carries four scalars, applied as transformers applies them, each 1 when left out: the
  embeddings are multiplied by `embedding_multiplier`, the attention scores by `attention_multiplier` (not divided
  by the square root of the head's width), what a block's attention and its MLP add to the residual by
  `residual_multiplier`, and the logits are divided by `logits_scaling`, which the shared readout does
  (`Architecture::logits_scaling`). candle-transformers 0.11.0's `granite` reads none of the four and scales by
  `1/sqrt(head_dim)`; its `granitemoehybrid` applies all four. granite-4.2-3b writes `attention_multiplier`
  0.015625 over heads 64 wide, where upstream's scale is 0.125, and 1.0 for the other three.
- `GraniteConfig` has no `tie_word_embeddings`: `granite_config` reads it, false when absent as in transformers
  (granite-4.2-3b has its own `lm_head.weight`).
- `Lfm2ForCausalLM`: `lfm2::Model::forward(input, 0, &mut cache)` returns the final-normed (`embedding_norm`) hidden
  states at every position. A layer is a short convolution or an attention by `layer_types` (`conv`,
  `full_attention`); the `Cache` holds the rotary tables, built once at load and cloned per prompt, and stores no
  keys, values or convolution states. A convolution layer pads the one whole prompt with `conv_L_cache - 1` zeros on
  its left and reads no state, which is exact for a prompt decoded alone at position 0; never decode an LFM2 prompt
  in pieces without `use_kv_cache`.
- The feed-forward width is transformers' (`Lfm2MLP`): `intermediate_size` as it is when `block_auto_adjust_ff_dim`
  is false, else two thirds of it times `block_ffn_dim_multiplier`, rounded up to `block_multiple_of`.
  candle-transformers 0.11.0 takes `hidden_size * 4` instead, which is LFM2.5-1.2B's 8192 but not LFM2.5-350M's
  4608 (from 6656), LFM2.5-230M's 2560 or LFM2.5-2.6B's 10752 (both as they are).
- The head is tied by `tie_embedding`, else by `tie_word_embeddings` (LFM2.5-2.6B writes that one alone), else
  tied, as in transformers; `Lfm2Config` reads `tie_embedding` alone. A tied head is `model.embed_tokens.weight`.
- `lfm2_config` refuses `conv_bias: true`: neither upstream nor the copy reads a bias of the convolution or its
  projections, and the LFM2.5 checkpoints write false. A `config.json` without `layer_types` and
  `intermediate_size` does not read: LFM2-350M and LFM2-700M, of the first generation, write `full_attn_idxs` and
  `block_ff_dim` instead. `Lfm2MoeForCausalLM` (LFM2.5-8B-A1B) has no implementation in candle and is refused at
  pull. LFM2.5-2.6B was not run (its `w1` is 10752 wide in the safetensors header, as the rule above gives).
- Checked against transformers 5.19.0 in F32 on the CPU (2026-10-11, by hand): the label logits through Candle are
  within 0.0001 of it for granite-4.2-3b at 28 and at 4.4k tokens, LFM2.5-1.2B-Instruct at 29 and at 4.4k tokens,
  and LFM2.5-350M and LFM2.5-230M (the two other feed-forward rules) at 29 and at 1.2k tokens.
- Both compute attention by matmul over `[heads, len, len]` scores cast to F32 on every device, so memory and time
  grow with the square of the prompt. In F32 on a 16-core x86 CPU granite-4.2-3b (40 layers of 40 heads, 7.3 GB of
  BF16 weights read as F32) answers the ticket in 24 s, its Q8_0 GGUF in 6 s, and holds 21.5 GB for a 28-token
  prompt; a load and two passes of 4.4k tokens took 30 minutes at a 29.9 GB peak (dev profile, Candle's crates
  optimised), so use the GGUF for a long state. LFM2.5-1.2B-Instruct answers the ticket's two rows over a
  4.9k-token state in 194 s, where its GGUF takes 83 s.
- candle-core 0.11.0 runs a grouped `conv1d` as one convolution per group, and LFM2's is depthwise: 2048 of them in
  each of LFM2.5-1.2B's 10 convolution layers. On a tensor of that shape one layer's convolution took 1.4 s at 80
  tokens and 1.9 s at 4.8k (2026-10-11, by hand), and the same sum as three shifted products 2 ms and 160 ms.
  LFM2.5-1.2B-Instruct answers the ticket (80 tokens) in 23 s, where its GGUF takes 1.6 s and Llama-3.2-1B through
  Candle 7.6 s. The copy keeps upstream's convolution.
- Not measured on Metal (no Apple machine, 2026-10-11): both keep the BF16 rule. Both compute their rotary tables
  and their attention in F32; LFM2's convolution runs in the model's dtype.

### Gemma 3 in the vendored module
- Two checkpoint forms, one module. `Gemma3ForCausalLM` (gemma-3-1b-it, `model_type` `gemma3_text`) writes a flat
  `config.json` and its tensors under `model.*`. `Gemma3ForConditionalGeneration` (gemma-3-4b-it and up,
  `model_type` `gemma3`) is a multimodal checkpoint: its text fields sit under `config.json#text_config` beside a
  `vision_config`, and its weight map holds the text tower under `language_model.model.*` (gemma-3-4b-it: 444 of 883
  tensors), SigLIP under `vision_tower.vision_model.*` (437) and `multi_modal_projector.*` (2). `Gemma3TextTower`
  reads `text_config` and, through `Architecture::tensor`, the `language_model.` tensors alone under the names a
  text-only checkpoint gives them, so `read_tensors` copies no vision tensor to the device and the decoder and the
  head read `model.*` either way. A checkpoint saved by transformers 4.52 or later writes the tower under
  `model.language_model.*` with its own `lm_head.weight`; that form was not run and is not read (the load fails
  naming `model.embed_tokens.weight`). The unsloth mirrors are the pins because google's repositories are gated.
- `gemma3::Model::forward(input, 0)` returns the final-normed hidden states at every position, with no key-value
  cache. The head is always the embedding (`model.embed_tokens.weight`; there is no
  `tie_word_embeddings`), the embeddings are multiplied by `sqrt(hidden_size)`, and every RmsNorm multiplies by
  `1 + weight`. Final-logit softcapping belongs to the readout (`Architecture::final_logit_softcapping`, "Logits"):
  both pinned checkpoints write `final_logit_softcapping: null` and `attn_logit_softcapping: null`, so on them
  nothing is softcapped; the readout's path is pinned by a unit test over random weights.
- A layer is sliding when `(layer_idx + 1) % sliding_window_pattern > 0` (five of every six); `sliding_window` is
  512 for 1B and 1024 for 4B. A sliding layer attends over the whole prompt under a mask that hides every key
  `sliding_window` or more positions before its query, which is transformers' `sliding_window_overlay` (`kv > q - window`) and
  llama.cpp's `is_masked_swa` (`p1 - p0 >= n_swa`); candle-transformers 0.11.0's mask hid one position less, a
  window of `sliding_window + 1`. A sliding layer rotates with `rope_local_base_freq` (10,000) unscaled; a global
  layer with `rope_theta` (1,000,000) divided by the `factor` of a `rope_scaling` of `rope_type` `linear`
  (gemma-3-4b-it writes 8, gemma-3-1b-it `null`), which upstream does not read and which llama.cpp applies as the
  GGUF's `rope.scaling.factor` to its non-SWA layers alone; another `rope_type` is refused at load. The attention
  scores are scaled by `query_pre_attn_scalar^-0.5` as transformers does (llama.cpp: `f_attention_scale`), where
  upstream scaled by `1/sqrt(head_dim)`: the same for 1B, 4B and 12B (256), not for 27B (168 against 128).
- `models::gemma3::Config` wants all 18 fields, `sliding_window_pattern` among them (both pins write 6 and no
  `layer_types`); a `config.json` that writes `layer_types` without the pattern, as transformers 4.54 and later
  save, does not read. Each layer builds its own rotary table over `max_position_embeddings` positions, as upstream
  does: 26 tables of 32,768 rows for 1B (0.9 GB in F32), 34 of 131,072 for 4B (4.6 GB). The decoder keeps no
  key-value cache, including the global layers; every call runs the whole prompt.
- Checked against transformers 5.19.0 in F32 on the CPU (2026-10-11, by hand, `Gemma3ForCausalLM` and
  `Gemma3ForConditionalGeneration` loaded from the same snapshots): the label logits through Candle are within
  0.0002 of it for gemma-3-1b-it and for gemma-3-4b-it, each at 42 and at 2.2k tokens, past both sliding windows,
  so what separates a checkpoint from its GGUF in those F32 checks is the quantization.
- Attention uses the shared query chunks in the model's dtype. Earlier unchunked F32 measurements on a 16-core
  x86 CPU (release build) had gemma-3-1b-it answer the ticket
  in 7.3 s, its Q8_0 GGUF through llama.cpp in 2.9 s, and the ticket's two rows over a 5.4k-token state in 148 s
  against the GGUF's 66 s; gemma-3-4b-it, whose text tower is 7.7 GB of BF16 read as F32, answers the ticket in
  31 s (the GGUF: 9.3 s) and the 5.4k-token state in 434 s (288 s), and holds 22.8 GB for a 42-token prompt and for
  one of 2.2k tokens.
- The rotary tables follow the F32-before-conversion rule above; attention keeps the BF16 rule on Metal and
  completes each query chunk before another can retain its temporary scores.

### Olmo 3 in the vendored module
- `Olmo3ForCausalLM` (Olmo-3-7B-Instruct, `model_type` `olmo3`) is OLMo 2's graph (post-norms, `q_norm` and `k_norm`
  over the whole projection width, an untied `lm_head.weight`) with two things candle-transformers 0.11.0's `olmo2`
  lacks, which `olmo2.rs` adds: `layer_types` and `rope_scaling`. `olmo2::Model::forward(input, 0)` returns the
  final-normed hidden states at every position; the module keeps no key-value cache, so the loaded decoder runs each
  prompt itself, nothing cloned or cleared.
- A layer is `config.json#layer_types[layer_idx]`: `sliding_attention` (three of every four in the 7B) attends under a
  mask that hides every key `sliding_window` (4096) or more positions before its query, transformers'
  `sliding_window_overlay` (`kv > q - window`) and llama.cpp's `is_masked_swa` (`p1 - p0 >= n_swa`), the rule W6 pinned
  for Gemma 3; `full_attention` attends over the whole prompt. Another type is refused, and so is a `config.json`
  without `layer_types` (transformers defaults to the 4-pattern and a window of 4096; the Olmo 3 checkpoints write
  both) or with a sliding layer and no `sliding_window`. The 7B has 32 key-value heads for its 32 heads: no grouping.
- `rope_scaling` (`rope_type` `yarn`: `factor` 8, `original_max_position_embeddings` 8192, `attention_factor`
  1.2079, `beta_fast` 32, `beta_slow` 1) scales the full-attention layers' rotary table alone, as transformers
  (`rope_parameters["full_attention"]`) and llama.cpp (`freq_scale` and `attn_factor` 1 on an SWA layer) have it; a
  sliding layer rotates with `rope_theta` (500,000) unscaled. The yarn table is transformers'
  `_compute_yarn_parameters`: a frequency pair that rotates `beta_fast` times or more over the original positions keeps
  its frequency, one rotating `beta_slow` times or fewer takes it divided by `factor`, a pair between (the range's ends
  floored and ceiled: pairs 18 to 35 of the 7B's 64) mixes the two linearly, and sin and cos are multiplied by
  `attention_factor` (`0.1 ln(factor) + 1` when left out, which is what the 7B writes). `null` or `default` scales
  nothing; another type, or `truncate: false`, is refused. llama.cpp reaches the same factor by another road (its
  `yarn_attn_factor` is cancelled and reapplied inside `rope_yarn`).
- Memory: the 7B's 7.3 billion parameters are 29.3 GB in F32, and candle's `[heads, len, len]` attention would add
  3.5 GB per score tensor and 5.5 GB of stored keys and values for a 5.2k-token prompt, past what this machine has
  beside the weights. The copy therefore uses the shared attention over 512 query rows at a time (each
  chunk's scores masked, softmaxed and multiplied by the values before the next, the chunks concatenated, the same
  numbers as one matmul) and stores no key-value cache (a prompt is decoded whole, at offset 0). On a 16-core x86 CPU
  (release build) the ticket's answers and the 2,490-token state's are identical to the unchunked copy's.
- In F32 on that CPU (release build) the 7B loads in 31 s, answers the ticket in 14 s (49 s with the load; its Q8_0
  GGUF through llama.cpp: 22 s), the 2,490-token state in 288 s and the suite's 4.9k-token state in 757 s against the
  GGUF's 563 s, time growing with the square of the prompt as for every other architecture. Its peak resident set
  is the load's, 36.9 GB for the ticket (the F32 weights beside the mapped BF16 pages, which the kernel can drop),
  and 38.0 GB for that 4,890-token state (729 s by hand), where the unchunked copy with a cache would have needed
  about 42 GB. Use the GGUF where that is too much.
- Checked against transformers 5.19.0 in F32 on the CPU (2026-10-12, by hand, `Olmo3ForCausalLM` from the same
  snapshot): the label logits through Candle are within 0.00003 of it at 28 tokens and within 0.00014 at 4,408 tokens,
  past the sliding window (slots at 1, 1469, 2204, 4406 and 4407), so what separates the checkpoint from its GGUF is
  the quantization. That 4.4k-token prompt took Candle 465 s at a 37.8 GB peak (dev profile, Candle's crates
  optimised; the peak is the load's, F32 weights beside the mapped BF16 pages) and transformers about 2 minutes.
- Both rotary tables follow the F32-before-conversion rule above, including the YaRN attention factor; attention
  stays in the model's dtype and completes each Metal query chunk.

### Logits
- Return label logits only: index the hidden states at the slots and the head's rows at the label ids
  (`index_select`), cast both to F32, multiply and divide by the architecture's `logits_scaling`, so a prompt never
  computes a whole vocabulary row. Check, as the
  llama.cpp runtime does, that the prompt is non-empty and within `n_ctx`, every slot inside it and every label id
  below `config.json#vocab_size`.
- The answers equal the GGUF of the same model through llama.cpp up to quantization: Qwen3-1.7B BF16 safetensors
  against `Qwen3-1.7B-Q8_0.gguf` gives the same top choices and noul sides on the ticket and sentiment fixtures, every
  probability within 0.1 (R1.2). `cargo xtask e2e candle` holds every other architecture to the same on the ticket,
  sentiment and score fixtures, in F32 on the CPU against the Q8_0 GGUF: Qwen2.5-1.5B-Instruct stays within 0.001
  (0.008 over a 5.4k-token state), SmolLM3-3B within 0.003, Llama-3.2-1B within 0.014, MiniCPM5-1B within 0.037 (a
  noul near 0.6), Phi-4-mini within 0.001 (0.0002 over a 4.9k-token state), granite-4.2-3b within 0.0001 (its
  answers to the three sit at 0 and 1), LFM2.5-1.2B-Instruct within 0.022 (a score level; 0.008 over a
  4.9k-token state), gemma-3-1b-it within 0.0001 (0.0009 over a 5.4k-token state) and gemma-3-4b-it within 0.0001
  (its answers to the three and to the 5.4k-token state sit at 0 and 1) and Olmo-3-7B-Instruct within 0.0001 on the
  three (its answers sit at 0 and 1) and 0.053 over a 4.9k-token state past its sliding window (a noul at 0.94 against
  the GGUF's 0.99).

## Testing
- `cargo test -p ardana-candle` reads only Qwen3-1.7B's `config.json` from `tmp/hf` (`cargo xtask fetch` installs
  it, leaving out `.safetensors`); the tests that load the checkpoint are `#[ignore]`d and run in the `qwen3:1.7b`
  row of `cargo xtask e2e rust`, which downloads hf.co/Qwen/Qwen3-1.7B at
  `70d244cc86ccca08cf5af4e1e306ecf908b1ad5e` and its GGUF, hf.co/Qwen/Qwen3-1.7B-GGUF `Qwen3-1.7B-Q8_0.gguf` at
  `90862c4b9d2787eaed51d12237eafdfe7c5f6077` (Q8), into `tmp/hf` and removes both after the row's tests, never an
  invented fixture.
- THX-01 has two plain unit tests in `crates/ardana-candle/src/lib.rs`: "thx01_reads_its_configs" (`encoder_config`
  from mmBERT's shape and from the flat thetas, the two refusals, the bucket temperatures and the clamp,
  `thx01::tensor`'s naming) and "thx01_computes_in_f32" (R8.5, R8.6: a model a few values wide over random weights
  saved as a checkpoint in `$ARDANA_TMP`, `config.json` naming `THXDecisionModel`, the action head and the
  temperature buffer beside its tensors; `CandleRuntime::supports` is true and `load` gives a `Loaded::Decision` at
  `gpu_layers` 0 and -1 alike, the tensors F32 both times, one probability per marker summing to 1, the same from
  the file as in memory and on a second call, the noul bucket at 5.0 flatter than at 0.5, and an empty sequence, a
  marker past the end and a sequence past the positions errors). "supports_the_architectures_config_json_names"
  covers the name. The real checkpoint is the suite's.
- `ops` pins long rotary positions (255/256/257 and 4096/4097/5483), F32 trig followed by BF16 conversion, and
  query-chunk equivalence to dense attention at 1/511/512/513/1025 queries with no mask, causal/sliding masks and
  attention softcapping. Native Apple silicon also runs these in Metal BF16 and checks retained Metal allocation
  immediately after repeated attention calls, before a CPU readback can release temporary scores.
- A change a vendored module carries gets a plain unit test over random weights (`candle_nn::VarMap`, a model a few
  values wide built through `configuration` and `Architecture::build`), which pins the change and nothing about a
  real model's answers: "llama_reads_a_written_head_dim" (the projections' shapes, a state at every position, each
  the one the prompt cut there has, a missing tensor as an error), "phi3_scales_longrope_by_the_prompt_length"
  (the rotary table read back through `apply_rotary_emb_qkv`), "granite_applies_its_multipliers" (the slot logits
  of a `CausalLm` built in memory move with each of the three multipliers and are divided by `logits_scaling`),
  "lfm2_sizes_its_feed_forward_as_transformers" (the width, the tying, a state at every position through
  convolution and attention layers), "gemma3_windows_its_sliding_layers" (a sliding layer's last state over a prompt
  longer than its window follows the tokens inside the window alone, a global layer's every token, the linear
  rope factor moves the global layers alone, another type is refused) and
  "gemma3_softcaps_the_logits_in_the_readout" (the slot logits of a `CausalLm` built in memory over a cap of 30 are
  `30 * tanh(x / 30)` of the ones without), "olmo3_sliding_layers_see_their_window" (two layers, `[sliding_attention,
  full_attention]` with a window of 8: a 16-token prompt's slot logits move from position 8 on when the window is set
  to 16 and an 8-token prompt's not at all; a 513-token prompt has the states its 512 have, past the query chunk)
  and "olmo3_scales_the_full_layers_by_yarn" (the yarn frequencies for Olmo-3-7B-Instruct's own parameters, the
  scaling moving the full layers alone, another type refused). "gemma3_multimodal_reads_the_text_tower_only" (R6.3) writes the 883
  names of gemma-3-4b-it's weight map, one value each, into a scratch safetensors file and reads it as a load reads
  the shards: the 444 text-tower tensors come out under `model.*` and nothing else. A `VarMap` starts a tensor
  read with `vb.get` at zero (LFM2's convolution kernel, Gemma 3's norms, which multiply by `1 + weight`): give it
  random values before a test relies on it. The real checkpoints stay the suite's.
- `crates/ardana/tests/e2e_registry.rs` pulls the checkpoint online from a loopback stand-in of the Hub
  (`common::HubStandIn`) that records each file asked for, over a hub cache holding links to the row's blobs, so the
  pull is checked file by file without copying weights.
- Every other architecture is verified by `cargo xtask e2e candle` (`xtask/src/candle.rs`) on one row of
  `xtask/src/e2e.rs#CANDLE_FIXTURES`: a checkpoint and the GGUF of the same model, each pinned to a commit. `xtask/fetch.toml`
  pins neither; the suite pulls a row from the Hub through the release `ardana`, checks it and removes its files
  before the next (`hub::remove_added`, the removal every suite shares; the disk does not hold them all), so these
  checkpoints have no `#[ignore]`d test. A row passes when the pull
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
  `Phi-4-mini-instruct.Q8_0.gguf` at `78eb92a46fc37e6b524df991ed9aca9bc6aa7b80` (`long_state`),
  ibm-granite/granite-4.2-3b at `e459acceac81e5fe67c07d9cfc72329a332e7eb1` against ibm-granite/granite-4.2-3b-GGUF
  `granite-4.2-3b-Q8_0.gguf` at `c40945d71cd90f249a56985e8155551a9188dc30`, LiquidAI/LFM2.5-1.2B-Instruct at
  `0f604ada3f766f9f257460c4c9f0b5d6f69d431b` against LiquidAI/LFM2.5-1.2B-Instruct-GGUF
  `LFM2.5-1.2B-Instruct-Q8_0.gguf` at `8ed288026e23958ad9dfa92d53ed773a8eee7125` (`long_state`),
  unsloth/gemma-3-1b-it at `5b11413a10db4e486ef16a20101fd028f8f2499c` against ggml-org/gemma-3-1b-it-GGUF
  `gemma-3-1b-it-Q8_0.gguf` at `f9c28bcd85737ffc5aef028638d3341d49869c27` (`long_state`), unsloth/gemma-3-4b-it
  at `bf46152c47f5dd20b896357cb51abc4c03b8ee8c` against ggml-org/gemma-3-4b-it-GGUF `gemma-3-4b-it-Q8_0.gguf` at
  `d0976223747697cb51e056d85c532013931fe52e` (`long_state`), and allenai/Olmo-3-7B-Instruct at
  `6e5971d9eba42665f5bd5a0fcf047f299ce1dccc` against unsloth/Olmo-3-7B-Instruct-GGUF `Olmo-3-7B-Instruct-Q8_0.gguf`
  at `a2a9ae53f5a95087616b3f45ca96e9d2631344ee` (`long_state`, past its 4,096-token sliding window), and doofz/THX-01
  at `bef2e2478be7acd79429324a658558a57552a2e8` (`gguf: None`) against the `thx01` package's goldens: the row's pull
  records runtime `candle` at the pin with the six files, `tokenizer/tokenizer.json` and the kind `decision`;
  `ardana run --json` answers the three fixtures with the golden's choices, every probability within 0.01 and the
  golden's `input_tokens`; the ticket's state repeated 300 times is refused naming the 1,024-token window and a
  25-option choice naming the 24-option limit; `--verbose` prints the model, the tokens and the timings and
  `ardana list` sizes the entry by `model.safetensors`; one `ardana serve` on the row's home then answers the three
  fixtures at `/v1/systemone` held to the goldens the same way (0.01, the golden's `input_tokens`) and the first
  again with identical probabilities, and `ardana ps` lists the model while that server holds it. What the server
  does around a decision model's decode (the listing, the load on the first request, `/health`, the refusals before
  any load, keep-alive and eviction) is `crates/ardana-server/tests/http.rs`'s, on a fake decision model
  (`axum.md`, "Testing").

## Sources
- https://github.com/huggingface/candle — README: pure-Rust ML framework, backends, feature flags
- https://github.com/huggingface/candle/tree/0.11.0 — the 0.11.0 release the pins name
- https://github.com/huggingface/candle/blob/0.11.0/candle-core/Cargo.toml — `metal`, `cuda`, `mkl`, `accelerate` features; `tokenizers` 0.22 with `onig`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/qwen3.rs — `Config`, `Model::forward`, private `embed_tokens` and `clear_kv_cache`, tied head
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/qwen2.rs — `Config`, `Model::forward` and its masks, the sliding window, the rotary table's dtype
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/smol/smollm3.rs — `Config::should_skip_rope`, `Model::forward`, `KvCache`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/llama.rs — the file `llama.rs` is copied from: `LlamaConfig`, `Cache`, `Llama::forward` and its last-position narrowing, the `unwrap()` in `Llama::load`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/phi3.rs — the file `phi3.rs` is copied from: `Config`, `RotaryEmbedding::new` and its `longrope` branch, `Model::forward`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/granite.rs — the file `granite.rs` is copied from: `GraniteConfig`, `Cache`, `Granite::forward` and its last-position narrowing, the `1/sqrt(head_dim)` scale, the `unwrap()` in `Granite::load`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/granitemoehybrid.rs — the four Granite scalars as candle applies them elsewhere: `attention_multiplier`, `embedding_multiplier`, `residual_multiplier`, `logits_scaling`
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/lfm2.rs — the file `lfm2.rs` is copied from: `Lfm2Config`, `compute_intermediate_size`, `ShortConv`, `Model::forward` and its last-position narrowing
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/gemma3.rs — the file `gemma3.rs` is copied from: `Config`, `prepare_decoder_attention_mask` and its sliding window, the `1/sqrt(head_dim)` scale, `Model::forward`, its last-position narrowing and its softcapping
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/olmo2.rs — the file `olmo2.rs` is copied from: `Config`, `RotaryEmbedding::new` (one unscaled table), `prepare_decoder_attention_mask` (causal alone), `Attention::forward` and its `kv_cache`, `Model::forward` and its last-position narrowing
- https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/modernbert.rs — `Config` (`global_rope_theta`, `local_rope_theta`, `layer_norm_eps`), `ModernBert::load` and its `model.*` names, `forward` and its F32 masks, `get_local_attention_mask`
- https://huggingface.co/docs/transformers/model_doc/modernbert — `ModernBertConfig`: `global_attn_every_n_layers`, `local_attention`, `norm_eps`, `norm_bias`, `attention_bias`, `mlp_bias`, `rope_parameters`
- https://github.com/huggingface/candle/blob/0.11.0/candle-nn/src/kv_cache.rs — `RotatingCache::append`: the whole source returned when one pass exceeds the window
- https://github.com/huggingface/candle/blob/0.11.0/LICENSE-APACHE — the license the vendored files are taken under
- https://huggingface.co/docs/transformers/internal/rope_utils — `longrope`, `llama3` and `yarn` rope scaling: `short_factor`, `long_factor`, `original_max_position_embeddings`, `attention_factor`, `beta_fast`, `beta_slow`
- https://huggingface.co/docs/transformers/model_doc/olmo3 — `Olmo3Config`: `layer_types`, `sliding_window`, `rope_scaling` of the full-attention layers alone; `Olmo3ForCausalLM`
- https://huggingface.co/docs/transformers/model_doc/phi3 — `Phi3Config`: `rope_parameters` (a `config.json`'s `rope_scaling`), `original_max_position_embeddings`
- https://huggingface.co/docs/transformers/model_doc/llama — `LlamaConfig`: `head_dim`, `rope_parameters` (a `config.json`'s `rope_scaling`), `tie_word_embeddings`
- https://huggingface.co/docs/transformers/model_doc/granite — `GraniteConfig`: `embedding_multiplier`, `attention_multiplier`, `residual_multiplier`, `logits_scaling`, each 1.0 by default
- https://huggingface.co/docs/transformers/model_doc/lfm2 — `Lfm2Config`: `intermediate_size`, `block_auto_adjust_ff_dim`, `block_ffn_dim_multiplier`, `block_multiple_of`, `layer_types`, `conv_L_cache`, `tie_word_embeddings`
- https://huggingface.co/docs/transformers/model_doc/gemma3 — `Gemma3TextConfig` (`query_pre_attn_scalar`, `sliding_window`, `sliding_window_pattern`, `layer_types`, `rope_local_base_freq`, `final_logit_softcapping`) and `Gemma3Config` (`text_config`, `vision_config`); `Gemma3ForCausalLM` and `Gemma3ForConditionalGeneration`
- https://docs.rs/candle-core/0.11.0/candle_core/enum.Device.html — `Device::Cpu`, `new_metal`, `is_metal`
- https://docs.rs/candle-core/0.11.0/candle_core/safetensors/struct.MmapedSafetensors.html — `multi`, `tensors`, `load`
- https://docs.rs/candle-nn/0.11.0/candle_nn/var_builder/type.VarBuilder.html — `from_tensors`, `from_mmaped_safetensors`, `get`
- https://docs.rs/candle-transformers/0.11.0/candle_transformers/models/qwen3/index.html — the Qwen3 module
- https://huggingface.co/docs/safetensors/index — the safetensors format
- https://huggingface.co/docs/safetensors/metadata_parsing — sharded checkpoints and `model.safetensors.index.json`
- https://huggingface.co/docs/transformers/main_classes/configuration — `config.json`, `architectures`, `tie_word_embeddings`, `max_position_embeddings`
