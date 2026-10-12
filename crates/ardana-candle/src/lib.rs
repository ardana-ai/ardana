//! The Candle runtime behind `ardana-core`'s runtime traits: safetensors causal LMs read from a Hugging Face snapshot
//! directory ([`ardana_core::snapshot`]), each architecture `config.json#architectures` names mapped to one
//! implementation ([`ARCHITECTURES`]), never chosen by a model name.
//!
//! A model computes on the CPU in F32; an Apple silicon build puts it on Metal in BF16 unless `gpu_layers` is 0. An
//! architecture is its configuration ([`Architecture`]) and its decoder ([`Decoder`]), which returns the hidden states
//! at every position; the readout is shared ([`CausalLm`]): the label logits are the head's rows for the label ids
//! applied to the hidden states at the slots, in F32, so a prompt never computes a whole vocabulary row.
//!
//! candle-transformers' Llama, Phi-3, Granite, LFM2, Gemma 3 and OLMo 2 narrow to the last position inside the model,
//! so [`llama`], [`phi3`], [`granite`], [`lfm2`], [`gemma3`] and [`olmo2`] (Olmo 3) are its 0.11.0 files, copied and
//! changed as each one's header lists.
//!
//! One architecture is no causal LM: [`thx01`] (`THXDecisionModel`) is the THX-01 decision model, candle-transformers'
//! ModernBERT under the `thx01` package's decision head, loaded as a [`ardana_core::runtime::DecisionModel`] in F32 on
//! every device.

mod gemma3;
mod granite;
mod lfm2;
mod llama;
mod olmo2;
mod phi3;
pub mod thx01;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use ardana_core::runtime::{LoadOptions, Loaded, LoadedModel, Runtime};
use ardana_core::snapshot;
use candle_core::safetensors::MmapedSafetensors;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::smol::smollm3;
use candle_transformers::models::{qwen2, qwen3};
use serde_json::Value;

/// The `config.json#architectures` names this runtime implements: the causal LMs, then the decision model.
pub const ARCHITECTURES: [&str; 11] = [
    "Qwen3ForCausalLM",
    "Qwen2ForCausalLM",
    "SmolLM3ForCausalLM",
    "LlamaForCausalLM",
    "Phi3ForCausalLM",
    "GraniteForCausalLM",
    "Lfm2ForCausalLM",
    "Gemma3ForCausalLM",
    "Gemma3ForConditionalGeneration",
    "Olmo3ForCausalLM",
    thx01::ARCHITECTURE,
];

/// Candle for safetensors snapshots, on Metal by default on Apple silicon.
#[derive(Debug, Default, Clone, Copy)]
pub struct CandleRuntime;

impl Runtime for CandleRuntime {
    fn id(&self) -> &'static str {
        "candle"
    }

    /// A directory whose `config.json` names an architecture of [`ARCHITECTURES`].
    fn supports(&self, weights: &Path) -> bool {
        read_config(weights).is_ok_and(|config| architecture(&config).is_some())
    }

    /// A causal LM as a [`Loaded::Causal`] in the device's precision; THX-01 as a [`Loaded::Decision`] in F32
    /// ([`thx01::DTYPE`]) on the same device.
    fn load(&self, weights: &Path, opts: &LoadOptions) -> Result<Loaded> {
        let config = read_config(weights)?;
        let Some(architecture) = architecture(&config) else {
            bail!(
                "{} names the architectures {:?}; candle implements {}",
                weights.join(snapshot::CONFIG).display(),
                snapshot::architectures(&config),
                ARCHITECTURES.join(", ")
            );
        };
        let (device, dtype) = device(opts.gpu_layers)?;
        if architecture == thx01::ARCHITECTURE {
            return Ok(Loaded::Decision(Box::new(thx01::Thx01::load(
                weights, &device,
            )?)));
        }
        let model = CausalLm::load(weights, architecture, &config, opts, &device, dtype)?;
        Ok(Loaded::Causal(Box::new(model)))
    }
}

/// `config.json` of the snapshot directory `dir`.
fn read_config(dir: &Path) -> Result<Value> {
    let path = dir.join(snapshot::CONFIG);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// The first architecture `config` names that this runtime implements.
fn architecture(config: &Value) -> Option<&'static str> {
    snapshot::architectures(config)
        .iter()
        .find_map(|name| ARCHITECTURES.iter().copied().find(|a| a == name))
}

/// Where a model computes and in which precision: the CPU in F32 at `gpu_layers` 0 and on every build without Metal;
/// otherwise (-1, or any count: Candle places a model whole) Metal in BF16.
fn device(gpu_layers: i32) -> Result<(Device, DType)> {
    match gpu_layers {
        0 => Ok((Device::Cpu, DType::F32)),
        n if n < -1 => bail!("gpu_layers must be -1 (all) or >= 0, got {n}"),
        _ => gpu(),
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn gpu() -> Result<(Device, DType)> {
    let device = Device::new_metal(0).context("opening the Metal device")?;
    Ok((device, DType::BF16))
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn gpu() -> Result<(Device, DType)> {
    Ok((Device::Cpu, DType::F32))
}

/// Every tensor of `files` that `tensor` names (an architecture's [`Architecture::tensor`], or [`thx01::tensor`]),
/// under the name it reads it by, on `device` in `dtype`, each converted once, so a tied head shares its embedding's
/// storage; a tensor it never reads (a multimodal checkpoint's vision tower, THX-01's action head) stays in the mapping.
fn read_tensors(
    files: &[PathBuf],
    tensor: impl Fn(&str) -> Option<String>,
    device: &Device,
    dtype: DType,
) -> Result<HashMap<String, Tensor>> {
    // SAFETY: the files are hub cache blobs, which are never written in place once downloaded
    // (docs/guidelines/huggingface.md, "Hub cache layout"), so the mapping does not change while it is read; every
    // tensor is copied out of it before it is dropped at the end of this function.
    let mapped = unsafe { MmapedSafetensors::multi(files) }
        .with_context(|| format!("mapping {}", display(files)))?;
    let mut tensors = HashMap::new();
    for (name, _) in mapped.tensors() {
        let Some(read_as) = tensor(&name) else {
            continue;
        };
        let tensor = mapped
            .load(&name, device)
            .and_then(|tensor| tensor.to_dtype(dtype))
            .with_context(|| format!("reading the tensor {name}"))?;
        tensors.insert(read_as, tensor);
    }
    Ok(tensors)
}

fn display(files: &[PathBuf]) -> String {
    let names: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
    names.join(", ")
}

/// What the readout reads of an architecture's configuration.
struct Shape {
    /// `vocab_size`: the output projection's rows.
    vocab: usize,
    /// `hidden_size`: its columns.
    hidden: usize,
    /// `max_position_embeddings`: where the rotary tables end.
    max_positions: usize,
    /// `tie_word_embeddings`: the output projection is the embedding.
    tied: bool,
}

/// One architecture's own configuration, read from `config.json` before any tensor.
trait Architecture {
    fn shape(&self) -> Shape;
    /// What the readout divides the logits by: 1, but for Granite's `logits_scaling`.
    fn logits_scaling(&self) -> f64 {
        1.0
    }
    /// What the readout softcaps the logits with, as `cap * tanh(logits / cap)`: none, but for Gemma 3's
    /// `final_logit_softcapping`.
    fn final_logit_softcapping(&self) -> Option<f64> {
        None
    }
    /// The name the decoder reads the checkpoint tensor `name` under, or `None` for one it never reads: every tensor
    /// under its own name, but for the text tower of a multimodal checkpoint.
    fn tensor<'a>(&self, name: &'a str) -> Option<&'a str> {
        Some(name)
    }
    /// The decoder over the checkpoint's tensors, rooted where `config.json` sits.
    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>>;
}

/// One architecture's decoder, without its output projection.
trait Decoder: Send {
    /// The final-normed hidden states `[1, len, hidden]` of the one prompt `input` (`[1, len]`), decoded alone: on a
    /// key-value cache that starts empty and is dropped with the call.
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor>;
}

/// The configuration of `architecture`, a name of [`ARCHITECTURES`], from `config`.
fn configuration(architecture: &str, config: &Value) -> Result<Box<dyn Architecture>> {
    Ok(match architecture {
        "Qwen3ForCausalLM" => Box::new(serde_json::from_value::<qwen3::Config>(config.clone())?),
        "Qwen2ForCausalLM" => Box::new(qwen2_config(config)?),
        "SmolLM3ForCausalLM" => {
            Box::new(serde_json::from_value::<smollm3::Config>(config.clone())?)
        }
        "LlamaForCausalLM" => Box::new(llama_config(config)?),
        "Phi3ForCausalLM" => Box::new(serde_json::from_value::<phi3::Config>(config.clone())?),
        "GraniteForCausalLM" => Box::new(granite_config(config)?),
        "Lfm2ForCausalLM" => Box::new(lfm2_config(config)?),
        "Gemma3ForCausalLM" => Box::new(gemma3_config(config)?),
        "Gemma3ForConditionalGeneration" => Box::new(Gemma3TextTower(
            gemma3_config(&config["text_config"]).context("text_config")?,
        )),
        "Olmo3ForCausalLM" => Box::new(olmo3_config(config)?),
        other => bail!("no implementation of {other}"),
    })
}

impl Architecture for qwen3::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_word_embeddings,
        }
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        Ok(Box::new(qwen3::Model::new(self, vb)?))
    }
}

/// Each prompt runs a clone of the loaded decoder: 0.11.0 keeps `Model::clear_kv_cache` private.
impl Decoder for qwen3::Model {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.clone().forward(input, 0)
    }
}

/// `config` as candle's Qwen2 configuration. 0.11.0 masks every layer by `sliding_window` whatever
/// `use_sliding_window` says, and reads no `null` there: a checkpoint that turns the window off, as Qwen2.5's do,
/// gets one as wide as its positions, which masks nothing; one that turns it on is refused, since transformers
/// windows only the layers from `max_window_layers` on.
fn qwen2_config(config: &Value) -> Result<qwen2::Config> {
    ensure!(
        config["use_sliding_window"] != Value::Bool(true),
        "use_sliding_window is true, and sliding-window attention is not implemented"
    );
    let mut config = config.clone();
    config["sliding_window"] = config["max_position_embeddings"].clone();
    Ok(serde_json::from_value(config)?)
}

impl Architecture for qwen2::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_word_embeddings,
        }
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        Ok(Box::new(qwen2::Model::new(self, vb)?))
    }
}

/// Without an attention mask of its own, `forward` builds the causal one.
impl Decoder for qwen2::Model {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.clone().forward(input, 0, None)
    }
}

impl Architecture for smollm3::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_word_embeddings,
        }
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        Ok(Box::new(smollm3::Model::new(self, vb)?))
    }
}

impl Decoder for smollm3::Model {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.clone().forward(input, 0)
    }
}

/// `config` as the vendored Llama configuration: candle's own fields, and the `head_dim` a checkpoint writes out
/// (MiniCPM5's is not `hidden_size / num_attention_heads`).
fn llama_config(config: &Value) -> Result<llama::Config> {
    let head_dim = serde_json::from_value(config["head_dim"].clone()).context("head_dim")?;
    let config = serde_json::from_value(config.clone())?;
    Ok(llama::Config::new(config, head_dim))
}

impl Architecture for llama::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_word_embeddings,
        }
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        let cache = llama::Cache::new(false, vb.dtype(), self, vb.device())?;
        let model = llama::Llama::load(vb, self)?;
        Ok(Box::new(Llama { model, cache }))
    }
}

/// Llama with the cache that holds its rotary tables, built once at load.
struct Llama {
    model: llama::Llama,
    cache: llama::Cache,
}

/// Each prompt runs on a clone of the loaded cache, which stores no keys or values.
impl Decoder for Llama {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.model.forward(input, 0, &mut self.cache.clone())
    }
}

impl Architecture for phi3::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_word_embeddings,
        }
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        Ok(Box::new(phi3::Model::new(self, vb)?))
    }
}

impl Decoder for phi3::Model {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.clone().forward(input, 0)
    }
}

/// `config` as the vendored Granite configuration: candle's own fields, the four scalars transformers applies, each
/// 1 where `config.json` leaves it out, and `tie_word_embeddings`, false without one (Granite 4.2 has its own
/// `lm_head.weight`).
fn granite_config(config: &Value) -> Result<granite::Config> {
    let scalar = |name: &'static str| -> Result<f64> {
        let scalar: Option<f64> = serde_json::from_value(config[name].clone()).context(name)?;
        Ok(scalar.unwrap_or(1.0))
    };
    let multipliers = granite::Multipliers {
        embedding: scalar("embedding_multiplier")?,
        attention: scalar("attention_multiplier")?,
        residual: scalar("residual_multiplier")?,
        logits_scaling: scalar("logits_scaling")?,
    };
    let tied: Option<bool> = serde_json::from_value(config["tie_word_embeddings"].clone())
        .context("tie_word_embeddings")?;
    let config = serde_json::from_value(config.clone())?;
    Ok(granite::Config::new(
        config,
        multipliers,
        tied.unwrap_or(false),
    ))
}

impl Architecture for granite::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_word_embeddings,
        }
    }

    fn logits_scaling(&self) -> f64 {
        self.multipliers.logits_scaling
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        let cache = granite::Cache::new(false, vb.dtype(), self, vb.device())?;
        let model = granite::Granite::load(vb, self)?;
        Ok(Box::new(Granite { model, cache }))
    }
}

/// Granite with the cache that holds its rotary tables, built once at load.
struct Granite {
    model: granite::Granite,
    cache: granite::Cache,
}

/// Each prompt runs on a clone of the loaded cache, which stores no keys or values.
impl Decoder for Granite {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.model.forward(input, 0, &mut self.cache.clone())
    }
}

/// `config` as the vendored LFM2 configuration: candle's own fields, the `intermediate_size` and
/// `block_auto_adjust_ff_dim` (true without one) the feed-forward width comes from, and the tying, which
/// transformers reads from `tie_embedding`, else from `tie_word_embeddings` (LFM2.5-2.6B writes that one alone),
/// else as true. `conv_bias: true` is refused: candle reads no bias of the convolution or its projections.
fn lfm2_config(config: &Value) -> Result<lfm2::Config> {
    ensure!(
        config["conv_bias"] != Value::Bool(true),
        "conv_bias is true, and the convolution's biases are not implemented"
    );
    let flag = |name: &'static str| -> Result<Option<bool>> {
        serde_json::from_value(config[name].clone()).context(name)
    };
    let tied = match flag("tie_embedding")? {
        Some(tied) => tied,
        None => flag("tie_word_embeddings")?.unwrap_or(true),
    };
    let auto_adjust_ff_dim = flag("block_auto_adjust_ff_dim")?.unwrap_or(true);
    let intermediate_size =
        serde_json::from_value(config["intermediate_size"].clone()).context("intermediate_size")?;
    let config = serde_json::from_value(config.clone())?;
    Ok(lfm2::Config::new(
        config,
        intermediate_size,
        auto_adjust_ff_dim,
        tied,
    ))
}

impl Architecture for lfm2::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_embedding,
        }
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        let cache = lfm2::Cache::new(false, vb.dtype(), self, vb.device())?;
        let model = lfm2::Model::new(self, vb)?;
        Ok(Box::new(Lfm2 { model, cache }))
    }
}

/// LFM2 with the cache that holds its rotary tables, built once at load.
struct Lfm2 {
    model: lfm2::Model,
    cache: lfm2::Cache,
}

/// Each prompt runs on a clone of the loaded cache, which stores no keys, values or convolution states: a
/// convolution layer pads the one whole prompt with zeros on its left, as it does at a sequence's start.
impl Decoder for Lfm2 {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.model.forward(input, 0, &mut self.cache.clone())
    }
}

/// `config` (a `Gemma3ForCausalLM` checkpoint's, or the `text_config` of a multimodal one) as the vendored Gemma 3
/// configuration: candle's own fields, and the factor of a `rope_scaling` of type `linear`, which divides the global
/// layers' rotary frequencies (gemma-3-4b-it writes 8) and is 1 without one; another type is refused.
fn gemma3_config(config: &Value) -> Result<gemma3::Config> {
    let scaling = &config["rope_scaling"];
    let rope_type = scaling["rope_type"].as_str().or(scaling["type"].as_str());
    let factor = match rope_type {
        Some("linear") => {
            serde_json::from_value(scaling["factor"].clone()).context("rope_scaling.factor")?
        }
        Some(other) if other != "default" => {
            bail!("rope_scaling of type {other} is not implemented")
        }
        _ => 1.0,
    };
    let config = serde_json::from_value(config.clone())?;
    Ok(gemma3::Config::new(config, factor))
}

/// Gemma 3 ties its head to the embedding and has no `tie_word_embeddings` to say so.
impl Architecture for gemma3::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: true,
        }
    }

    fn final_logit_softcapping(&self) -> Option<f64> {
        self.final_logit_softcapping
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        Ok(Box::new(gemma3::Model::new(self, vb)?))
    }
}

/// The text tower of a `Gemma3ForConditionalGeneration` checkpoint: `text_config`'s Gemma 3, over the tensors under
/// `language_model.`, read under the names a `Gemma3ForCausalLM` checkpoint gives them; the vision tower's and the
/// projector's are never read.
struct Gemma3TextTower(gemma3::Config);

impl Architecture for Gemma3TextTower {
    fn shape(&self) -> Shape {
        self.0.shape()
    }

    fn final_logit_softcapping(&self) -> Option<f64> {
        self.0.final_logit_softcapping()
    }

    fn tensor<'a>(&self, name: &'a str) -> Option<&'a str> {
        name.strip_prefix("language_model.")
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        self.0.build(vb)
    }
}

/// Each prompt runs a clone of the loaded decoder, whose key-value caches start empty; a sliding layer attends over
/// the whole prompt under its window's mask.
impl Decoder for gemma3::Model {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.clone().forward(input, 0)
    }
}

/// `config` as the vendored Olmo 3 configuration: candle's own OLMo 2 fields, each layer's attention from
/// `layer_types` (`sliding_attention` or `full_attention`; another is refused), the `sliding_window` a sliding layer
/// needs, and the `rope_scaling` of the full layers: `yarn`, read as transformers reads it (`attention_factor`
/// `0.1 ln(factor) + 1` without one, `beta_fast` 32 and `beta_slow` 1 without them), or none (`null`, or `default`);
/// another type, or a `truncate` of false, is refused.
fn olmo3_config(config: &Value) -> Result<olmo2::Config> {
    let layer_types: Vec<String> =
        serde_json::from_value(config["layer_types"].clone()).context("layer_types")?;
    let layer_types = layer_types
        .iter()
        .map(|layer_type| match layer_type.as_str() {
            "sliding_attention" => Ok(olmo2::LayerType::SlidingAttention),
            "full_attention" => Ok(olmo2::LayerType::FullAttention),
            other => bail!("layer_types names {other}, and only sliding_attention and full_attention are implemented"),
        })
        .collect::<Result<Vec<_>>>()?;
    let sliding_window: Option<usize> =
        serde_json::from_value(config["sliding_window"].clone()).context("sliding_window")?;
    ensure!(
        sliding_window.is_some() || !layer_types.contains(&olmo2::LayerType::SlidingAttention),
        "layer_types names sliding_attention, and there is no sliding_window"
    );
    let scaling = &config["rope_scaling"];
    let rope_type = scaling["rope_type"].as_str().or(scaling["type"].as_str());
    let rope_scaling = match rope_type {
        Some("yarn") => {
            let number = |name: &'static str| -> Result<Option<f64>> {
                serde_json::from_value(scaling[name].clone())
                    .with_context(|| format!("rope_scaling.{name}"))
            };
            ensure!(
                scaling["truncate"] != Value::Bool(false),
                "rope_scaling.truncate is false, and only a truncated correction range is implemented"
            );
            let factor = number("factor")?.context("rope_scaling.factor")?;
            Some(olmo2::Yarn {
                factor,
                original_max_position_embeddings: serde_json::from_value(
                    scaling["original_max_position_embeddings"].clone(),
                )
                .context("rope_scaling.original_max_position_embeddings")?,
                attention_factor: number("attention_factor")?.unwrap_or(0.1 * factor.ln() + 1.0),
                beta_fast: number("beta_fast")?.unwrap_or(32.0),
                beta_slow: number("beta_slow")?.unwrap_or(1.0),
            })
        }
        Some(other) if other != "default" => {
            bail!("rope_scaling of type {other} is not implemented")
        }
        _ => None,
    };
    let config = serde_json::from_value(config.clone())?;
    Ok(olmo2::Config::new(
        config,
        layer_types,
        sliding_window,
        rope_scaling,
    ))
}

impl Architecture for olmo2::Config {
    fn shape(&self) -> Shape {
        Shape {
            vocab: self.vocab_size,
            hidden: self.hidden_size,
            max_positions: self.max_position_embeddings,
            tied: self.tie_word_embeddings,
        }
    }

    fn build(&self, vb: VarBuilder<'static>) -> candle_core::Result<Box<dyn Decoder>> {
        Ok(Box::new(olmo2::Model::new(self, vb)?))
    }
}

/// The vendored module keeps no key-value cache (a prompt is decoded whole, and a 7B model's would hold gigabytes), so
/// the loaded decoder runs each prompt itself, with nothing to clear or clone; a sliding layer attends over the whole
/// prompt under its window's mask.
impl Decoder for olmo2::Model {
    fn hidden_states(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.forward(input, 0)
    }
}

/// A loaded causal LM of any architecture: its decoder and the readout every architecture shares.
pub struct CausalLm {
    architecture: &'static str,
    decoder: Box<dyn Decoder>,
    /// The output projection, `[vocab, hidden]`: `lm_head.weight`, or the embedding it is tied to.
    head: Tensor,
    /// What the head's logits are divided by ([`Architecture::logits_scaling`]).
    logits_scaling: f64,
    /// What they are softcapped with after that ([`Architecture::final_logit_softcapping`]).
    softcapping: Option<f64>,
    vocab: usize,
    n_ctx: usize,
    dir: PathBuf,
}

impl std::fmt::Debug for CausalLm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CausalLm")
            .field("architecture", &self.architecture)
            .field("dir", &self.dir)
            .field("n_ctx", &self.n_ctx)
            .finish()
    }
}

impl CausalLm {
    fn load(
        dir: &Path,
        architecture: &'static str,
        config: &Value,
        opts: &LoadOptions,
        device: &Device,
        dtype: DType,
    ) -> Result<CausalLm> {
        let configuration = configuration(architecture, config).with_context(|| {
            format!(
                "{} is no {architecture} configuration",
                dir.join(snapshot::CONFIG).display()
            )
        })?;
        let shape = configuration.shape();
        let files = snapshot::weight_files(dir).map_err(anyhow::Error::msg)?;
        let tensors = read_tensors(
            &files,
            |name| configuration.tensor(name).map(str::to_string),
            device,
            dtype,
        )?;
        let vb = VarBuilder::from_tensors(tensors, dtype, device);
        let decoder = configuration
            .build(vb.clone())
            .with_context(|| format!("building {architecture} from {}", dir.display()))?;
        let head = if shape.tied {
            "model.embed_tokens.weight"
        } else {
            "lm_head.weight"
        };
        let head = vb
            .get((shape.vocab, shape.hidden), head)
            .with_context(|| format!("reading the output projection {head}"))?;
        let n_ctx = (opts.n_ctx as usize).min(shape.max_positions);
        ensure!(n_ctx > 0, "n_ctx must be greater than 0");
        Ok(CausalLm {
            architecture,
            decoder,
            head,
            logits_scaling: configuration.logits_scaling(),
            softcapping: configuration.final_logit_softcapping(),
            vocab: shape.vocab,
            n_ctx,
            dir: dir.to_path_buf(),
        })
    }
}

impl LoadedModel for CausalLm {
    /// The smaller of the asked window and `config.json#max_position_embeddings` (Q11).
    fn n_ctx(&self) -> usize {
        self.n_ctx
    }

    fn slot_logits(
        &mut self,
        ids: &[u32],
        slots: &[usize],
        label_ids: &[u32],
    ) -> Result<Vec<Vec<f32>>> {
        ensure!(!ids.is_empty(), "an empty prompt has no answer slot");
        ensure!(
            ids.len() <= self.n_ctx,
            "a prompt of {} tokens exceeds the context window of {} tokens",
            ids.len(),
            self.n_ctx
        );
        if let Some(&bad) = label_ids.iter().find(|&&id| id as usize >= self.vocab) {
            bail!(
                "label id {bad} is outside the model's {}-token vocabulary",
                self.vocab
            );
        }
        if let Some(&bad) = slots.iter().find(|&&s| s >= ids.len()) {
            bail!("slot {bad} is outside the {}-token prompt", ids.len());
        }
        if slots.is_empty() || label_ids.is_empty() {
            return Ok(vec![Vec::new(); slots.len()]);
        }
        let device = self.head.device();
        let slots = slots
            .iter()
            .map(|&s| u32::try_from(s).context("a slot does not fit a u32"))
            .collect::<Result<Vec<u32>>>()?;
        let input = Tensor::new(ids, device)?.unsqueeze(0)?;
        let hidden = self.decoder.hidden_states(&input)?.squeeze(0)?;
        let at_slots = hidden
            .index_select(&Tensor::new(slots.as_slice(), device)?, 0)?
            .to_dtype(DType::F32)?;
        let labels = self
            .head
            .index_select(&Tensor::new(label_ids, device)?, 0)?
            .to_dtype(DType::F32)?;
        let logits = (at_slots.matmul(&labels.t()?)? / self.logits_scaling)?;
        // Softcapping is elementwise, so the label rows alone softcap as the whole vocabulary row would.
        let logits = match self.softcapping {
            None => logits,
            Some(cap) => ((logits / cap)?.tanh()? * cap)?,
        };
        Ok(logits.to_vec2::<f32>()?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The pinned Qwen/Qwen3-1.7B snapshot in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
    fn qwen3_snapshot() -> PathBuf {
        let repo = PathBuf::from(std::env::var_os("HF_HOME").expect("cargo sets HF_HOME"))
            .join("hub/models--Qwen--Qwen3-1.7B");
        let commit = std::fs::read_to_string(repo.join("refs/main"))
            .unwrap_or_else(|err| panic!("{}: {err}; run `cargo xtask fetch`", repo.display()));
        repo.join("snapshots").join(commit.trim())
    }

    /// `$ARDANA_TMP/<test>`, emptied.
    fn scratch(test: &str) -> PathBuf {
        let dir = PathBuf::from(std::env::var_os("ARDANA_TMP").expect("cargo sets ARDANA_TMP"))
            .join(test);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Q2, Q3: a directory is supported by what its `config.json` names, never by its name; a weights file, a directory
    /// without `config.json` and an architecture without an implementation are not.
    #[test]
    fn supports_the_architectures_config_json_names() {
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(qwen3_snapshot().join(snapshot::CONFIG)).unwrap(),
        )
        .unwrap();
        let dir = scratch("candle-supports");
        let write = |name: &str, architectures: Value| {
            let snapshot = dir.join(name);
            std::fs::create_dir_all(&snapshot).unwrap();
            let mut config = config.clone();
            config["architectures"] = architectures;
            std::fs::write(snapshot.join(snapshot::CONFIG), config.to_string()).unwrap();
            snapshot
        };
        let runtime = CandleRuntime;
        assert!(runtime.supports(&write("mamba-named-qwen3", json!(["Qwen3ForCausalLM"]))));
        assert!(runtime.supports(&write("two", json!(["Other", "Qwen3ForCausalLM"]))));
        for architecture in [
            "Qwen2ForCausalLM",
            "SmolLM3ForCausalLM",
            "LlamaForCausalLM",
            "Phi3ForCausalLM",
            "GraniteForCausalLM",
            "Lfm2ForCausalLM",
            "Gemma3ForCausalLM",
            "Gemma3ForConditionalGeneration",
            "Olmo3ForCausalLM",
            "THXDecisionModel",
        ] {
            assert!(ARCHITECTURES.contains(&architecture));
            assert!(runtime.supports(&write(architecture, json!([architecture]))));
        }
        assert!(!runtime.supports(&write("qwen3-named-mamba", json!(["MambaForCausalLM"]))));
        assert!(!runtime.supports(&write("none", Value::Null)));
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(!runtime.supports(&empty));
        let gguf = dir.join("model.gguf");
        std::fs::write(&gguf, b"GGUF\x03\x00\x00\x00").unwrap();
        assert!(!runtime.supports(&gguf));
        let Err(err) = runtime.load(&dir.join("qwen3-named-mamba"), &LoadOptions::default()) else {
            panic!("an unimplemented architecture loaded");
        };
        assert!(
            err.to_string().contains(
                "[\"MambaForCausalLM\"]; candle implements Qwen3ForCausalLM, Qwen2ForCausalLM, SmolLM3ForCausalLM, \
                 LlamaForCausalLM, Phi3ForCausalLM, GraniteForCausalLM, Lfm2ForCausalLM, Gemma3ForCausalLM, \
                 Gemma3ForConditionalGeneration, Olmo3ForCausalLM, THXDecisionModel"
            ),
            "{err:#}"
        );
    }

    /// Qwen2's window: off, it is as wide as the positions whatever `sliding_window` says (a number below them, or
    /// `null`), so candle's mask hides no earlier token; on, the configuration is refused.
    #[test]
    fn qwen2_reads_no_sliding_window() {
        let config = |sliding_window: Value, use_sliding_window: bool| {
            json!({
                "vocab_size": 151_936,
                "hidden_size": 1536,
                "intermediate_size": 8960,
                "num_hidden_layers": 28,
                "num_attention_heads": 12,
                "num_key_value_heads": 2,
                "max_position_embeddings": 32_768,
                "sliding_window": sliding_window,
                "max_window_layers": 21,
                "tie_word_embeddings": true,
                "rope_theta": 1_000_000.0,
                "rms_norm_eps": 1e-6,
                "use_sliding_window": use_sliding_window,
                "hidden_act": "silu",
            })
        };
        for sliding_window in [json!(4096), Value::Null] {
            let read = qwen2_config(&config(sliding_window, false)).unwrap();
            assert_eq!(read.sliding_window, 32_768);
            assert_eq!(read.shape().max_positions, 32_768);
        }
        let err = qwen2_config(&config(json!(4096), true)).unwrap_err();
        assert!(err.to_string().contains("use_sliding_window"), "{err:#}");
    }

    /// The decoder of `architecture` for `config` over random weights, and those weights by name.
    fn random_decoder(
        architecture: &str,
        config: &Value,
    ) -> (Box<dyn Decoder>, HashMap<String, Tensor>) {
        let weights = candle_nn::VarMap::new();
        let vb = VarBuilder::from_varmap(&weights, DType::F32, &Device::Cpu);
        let decoder = configuration(architecture, config)
            .unwrap()
            .build(vb)
            .unwrap();
        let weights = weights.data().lock().unwrap();
        let weights = weights
            .iter()
            .map(|(name, var)| (name.clone(), var.as_tensor().clone()))
            .collect();
        (decoder, weights)
    }

    /// The hidden states of the prompt `ids`, one row per position.
    fn hidden(decoder: &dyn Decoder, ids: &[u32]) -> Vec<Vec<f32>> {
        let input = Tensor::new(ids, &Device::Cpu).unwrap().unsqueeze(0);
        let states = decoder.hidden_states(&input.unwrap()).unwrap();
        states.squeeze(0).unwrap().to_vec2().unwrap()
    }

    /// What the readout asks of a vendored decoder: a state of `width` at every position of `ids`, each the one the
    /// prompt without its last token has there, and the same states from a second call.
    fn assert_every_position(decoder: &dyn Decoder, ids: &[u32], width: usize) {
        let states = hidden(decoder, ids);
        assert_eq!((states.len(), states[0].len()), (ids.len(), width));
        let shorter = hidden(decoder, &ids[..ids.len() - 1]);
        for (position, (whole, cut)) in states.iter().zip(&shorter).enumerate() {
            for (a, b) in whole.iter().zip(cut) {
                assert!((a - b).abs() < 1e-4, "position {position}: {a} and {b}");
            }
        }
        assert_eq!(hidden(decoder, ids), states);
    }

    /// The vendored Llama over random weights: `head_dim` is the one `config.json` writes out (MiniCPM5: 128, with
    /// 1536 hidden and 16 heads) and sizes the projections, else `hidden_size / num_attention_heads`; every position
    /// has its state; a layer tensor the checkpoint lacks is an error.
    #[test]
    fn llama_reads_a_written_head_dim() {
        let mut config = json!({
            "hidden_size": 12,
            "intermediate_size": 16,
            "vocab_size": 20,
            "num_hidden_layers": 2,
            "num_attention_heads": 4,
            "num_key_value_heads": 2,
            "head_dim": 8,
            "rms_norm_eps": 1e-6,
            "rope_theta": 5_000_000,
            "rope_scaling": null,
            "max_position_embeddings": 32,
            "tie_word_embeddings": false,
            "eos_token_id": [1, 2],
        });
        assert_eq!(llama_config(&config).unwrap().head_dim, 8);
        let (decoder, weights) = random_decoder("LlamaForCausalLM", &config);
        let dims = |name: &str| weights[&format!("model.layers.1.self_attn.{name}.weight")].dims();
        assert_eq!(dims("q_proj"), [32, 12]);
        assert_eq!(dims("k_proj"), [16, 12]);
        assert_eq!(dims("o_proj"), [12, 32]);
        assert_every_position(decoder.as_ref(), &[3, 1, 4, 1, 5], 12);

        let lacking = "model.layers.1.mlp.up_proj.weight";
        let mut tensors = weights.clone();
        tensors.remove(lacking).unwrap();
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &Device::Cpu);
        let Err(err) = llama_config(&config).unwrap().build(vb) else {
            panic!("a checkpoint without {lacking} loaded");
        };
        assert!(err.to_string().contains(lacking), "{err}");

        config.as_object_mut().unwrap().remove("head_dim");
        assert_eq!(llama_config(&config).unwrap().head_dim, 3);
    }

    /// The vendored Phi-3 over random weights, with Phi-4-mini's `longrope` as transformers computes it: a frequency
    /// is divided by its factor, a prompt longer than `original_max_position_embeddings` reads the long factors at
    /// every position and any other the short ones, and sin and cos carry the attention factor
    /// `sqrt(1 + ln(max / original) / ln(original))`; the dimensions past `partial_rotary_factor` do not rotate.
    #[test]
    fn phi3_scales_longrope_by_the_prompt_length() {
        let config = json!({
            "vocab_size": 20,
            "hidden_act": "silu",
            "hidden_size": 16,
            "intermediate_size": 16,
            "num_hidden_layers": 2,
            "num_attention_heads": 2,
            "num_key_value_heads": 1,
            "rms_norm_eps": 1e-5,
            "rope_theta": 10000.0,
            "bos_token_id": 1,
            "eos_token_id": 2,
            "rope_scaling": {"type": "longrope", "short_factor": [1.0, 1.0], "long_factor": [1, 4.0]},
            "max_position_embeddings": 16,
            "original_max_position_embeddings": 4,
            "partial_rotary_factor": 0.5,
            "tie_word_embeddings": true,
        });
        let read: phi3::Config = serde_json::from_value(config.clone()).unwrap();
        let rotary = phi3::RotaryEmbedding::new(DType::F32, &read, &Device::Cpu).unwrap();
        // A head is 8 wide and its first 4 dimensions rotate, as the pairs (0, 2) and (1, 3): a pair that starts as
        // (1, 0) comes back as its frequency's (cos, sin) at the position.
        let rotated = |len: usize| -> Vec<Vec<f32>> {
            let head = Tensor::new(&[[1f32, 1., 0., 0., 7., 7., 7., 7.]], &Device::Cpu).unwrap();
            let heads = head.repeat((len, 1)).unwrap().reshape((1, 1, len, 8));
            let heads = heads.unwrap();
            let (rotated, _) = rotary.apply_rotary_emb_qkv(&heads, &heads, 0).unwrap();
            rotated.reshape((len, 8)).unwrap().to_vec2().unwrap()
        };
        // The frequencies are 1 and 10000^(-2/4); the attention factor is sqrt(1 + ln(16 / 4) / ln 4).
        let scale = 2f32.sqrt();
        for (len, factor) in [(4, 1f32), (5, 4.)] {
            let (first, second) = (3f32, 3. * 0.01 / factor);
            let want = [first.cos(), second.cos(), first.sin(), second.sin()].map(|x| scale * x);
            let got = &rotated(len)[3];
            for (got, want) in got.iter().zip(want.iter().chain(&[7.; 4])) {
                assert!((got - want).abs() < 1e-5, "{len} tokens: {got}, not {want}");
            }
        }

        // Four tokens and their prefix read the short factors, six and theirs the long ones.
        let (decoder, _) = random_decoder("Phi3ForCausalLM", &config);
        assert_every_position(decoder.as_ref(), &[3, 1, 4, 1], 16);
        assert_every_position(decoder.as_ref(), &[3, 1, 4, 1, 5, 9], 16);
    }

    /// R5.3, the vendored Granite over random weights: two layers with heads 64 wide, whose attention scores
    /// candle-transformers 0.11.0 multiplies by 1/sqrt(64) = 0.125 whatever `config.json` says. The slot logits
    /// follow `attention_multiplier` (Granite 4.2 writes 0.015625), `embedding_multiplier` and
    /// `residual_multiplier`, and are divided by `logits_scaling`; a scalar `config.json` leaves out is 1; every
    /// position has its state; a layer tensor the checkpoint lacks is an error.
    #[test]
    fn granite_applies_its_multipliers() {
        let config = |scalars: Value| {
            let mut config = json!({
                "hidden_size": 128,
                "intermediate_size": 16,
                "vocab_size": 20,
                "num_hidden_layers": 2,
                "num_attention_heads": 2,
                "num_key_value_heads": 1,
                "rms_norm_eps": 1e-5,
                "rope_theta": 10_000_000,
                "rope_scaling": null,
                "max_position_embeddings": 32,
                "tie_word_embeddings": false,
                "eos_token_id": 1,
            });
            let scalars = scalars.as_object().unwrap().clone();
            config.as_object_mut().unwrap().extend(scalars);
            config
        };
        let upstream = config(json!({"attention_multiplier": 0.125}));
        let (decoder, weights) = random_decoder("GraniteForCausalLM", &upstream);
        assert_every_position(decoder.as_ref(), &[3, 1, 4, 1, 5], 128);

        let head = Tensor::randn(0f32, 1., (20, 128), &Device::Cpu).unwrap();
        let logits = |scalars: Value| -> Vec<f32> {
            let configuration = granite_config(&config(scalars)).unwrap();
            let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
            let mut model = CausalLm {
                architecture: "GraniteForCausalLM",
                decoder: configuration.build(vb).unwrap(),
                head: head.clone(),
                logits_scaling: configuration.logits_scaling(),
                softcapping: None,
                vocab: 20,
                n_ctx: 32,
                dir: PathBuf::new(),
            };
            let logits = model.slot_logits(&[3, 1, 4, 1, 5], &[2, 4], &[0, 7, 19]);
            logits.unwrap().concat()
        };
        let wide = logits(json!({"attention_multiplier": 0.125}));
        for scalars in [
            json!({"attention_multiplier": 0.015625}),
            json!({"attention_multiplier": 0.125, "embedding_multiplier": 12.0}),
            json!({"attention_multiplier": 0.125, "residual_multiplier": 0.22}),
        ] {
            let gap = widest_gap(&wide, &logits(scalars.clone()));
            assert!(gap > 0.01, "{scalars}: the logits moved by {gap}");
        }
        let scaled = logits(json!({"attention_multiplier": 0.125, "logits_scaling": 8.0}));
        assert_eq!(scaled, wide.iter().map(|x| x / 8.).collect::<Vec<_>>());
        let ones = json!({
            "attention_multiplier": 1,
            "embedding_multiplier": 1.0,
            "residual_multiplier": 1.0,
            "logits_scaling": 1.0,
        });
        assert_eq!(logits(json!({})), logits(ones));

        let lacking = "model.layers.1.mlp.up_proj.weight";
        let mut tensors = weights.clone();
        tensors.remove(lacking).unwrap();
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &Device::Cpu);
        let Err(err) = granite_config(&upstream).unwrap().build(vb) else {
            panic!("a checkpoint without {lacking} loaded");
        };
        assert!(err.to_string().contains(lacking), "{err}");
    }

    /// The vendored LFM2 over random weights: the feed-forward width is the one transformers computes from
    /// `intermediate_size` (two thirds of it, rounded up to `block_multiple_of`, or the size as it is when
    /// `block_auto_adjust_ff_dim` is false), where candle-transformers 0.11.0 takes four times the hidden size; the
    /// head is tied by `tie_embedding`, else by `tie_word_embeddings`, else tied; every position has its state
    /// through convolution and attention layers, a one-token prompt's included; `conv_bias` is refused.
    #[test]
    fn lfm2_sizes_its_feed_forward_as_transformers() {
        let mut config = json!({
            "vocab_size": 20,
            "hidden_size": 16,
            "intermediate_size": 100,
            "block_auto_adjust_ff_dim": true,
            "block_ffn_dim_multiplier": 1.0,
            "block_multiple_of": 8,
            "num_hidden_layers": 3,
            "num_attention_heads": 4,
            "num_key_value_heads": 2,
            "norm_eps": 1e-5,
            "max_position_embeddings": 32,
            "conv_L_cache": 3,
            "conv_bias": false,
            "layer_types": ["conv", "full_attention", "conv"],
            "tie_embedding": true,
            "eos_token_id": 7,
        });
        assert_eq!(lfm2_config(&config).unwrap().intermediate_size, 72);
        let (_, mut weights) = random_decoder("Lfm2ForCausalLM", &config);
        let width = weights["model.layers.0.feed_forward.w1.weight"].dims()[0];
        assert_eq!(width, 72);
        // A `VarMap` starts a convolution's kernel at zero: both layers get one that convolves.
        for layer in [0, 2] {
            let name = format!("model.layers.{layer}.conv.conv.weight");
            let kernel = Tensor::randn(0f32, 1., (16, 1, 3), &Device::Cpu).unwrap();
            let zeros = weights.insert(name, kernel).unwrap();
            assert_eq!(
                zeros.flatten_all().unwrap().to_vec1::<f32>().unwrap(),
                [0.; 48]
            );
        }
        let vb = VarBuilder::from_tensors(weights, DType::F32, &Device::Cpu);
        let decoder = lfm2_config(&config).unwrap().build(vb).unwrap();
        assert_every_position(decoder.as_ref(), &[3, 1, 4, 1, 5], 16);
        assert_every_position(decoder.as_ref(), &[3, 1], 16);

        config["block_auto_adjust_ff_dim"] = json!(false);
        assert_eq!(lfm2_config(&config).unwrap().intermediate_size, 100);
        let fields = config.as_object_mut().unwrap();
        fields.insert("tie_word_embeddings".into(), json!(false));
        assert!(lfm2_config(&config).unwrap().tie_embedding);
        config.as_object_mut().unwrap().remove("tie_embedding");
        assert!(!lfm2_config(&config).unwrap().tie_embedding);
        config
            .as_object_mut()
            .unwrap()
            .remove("tie_word_embeddings");
        assert!(lfm2_config(&config).unwrap().tie_embedding);

        config["conv_bias"] = json!(true);
        let err = lfm2_config(&config).unwrap_err();
        assert!(err.to_string().contains("conv_bias"), "{err:#}");
    }

    /// A Gemma 3 `config.json` a few values wide: `layers` layers, a window of `sliding_window` tokens, and the layers
    /// sliding by `sliding_window_pattern` (every layer with a pattern above their count, none with 1).
    fn gemma3_config_json(layers: usize, sliding_window: usize, pattern: usize) -> Value {
        json!({
            "attention_bias": false,
            "attn_logit_softcapping": null,
            "final_logit_softcapping": null,
            "head_dim": 8,
            "hidden_activation": "gelu_pytorch_tanh",
            "hidden_size": 16,
            "intermediate_size": 24,
            "max_position_embeddings": 32,
            "num_attention_heads": 2,
            "num_hidden_layers": layers,
            "num_key_value_heads": 2,
            "query_pre_attn_scalar": 8,
            "rms_norm_eps": 1e-6,
            "rope_local_base_freq": 10000.0,
            "rope_scaling": null,
            "rope_theta": 1000000.0,
            "sliding_window": sliding_window,
            "sliding_window_pattern": pattern,
            "vocab_size": 20,
        })
    }

    /// The widest gap between two rows.
    fn widest_gap(a: &[f32], b: &[f32]) -> f32 {
        let gaps = a.iter().zip(b).map(|(a, b)| (a - b).abs());
        gaps.fold(0f32, f32::max)
    }

    /// The vendored Gemma 3 over random weights: one sliding layer with a window of 2 reads a prompt longer than the
    /// window (candle's rotating cache hands the whole prompt back, and the mask windows it), and a position's state
    /// follows its own token and the one before, never the one two back, as transformers and llama.cpp window it
    /// (upstream's mask let that one in); a global layer follows every token. The global layers alone divide their
    /// rotary frequencies by a linear `rope_scaling`'s factor, which leaves position 0 as it was; another type of
    /// scaling is refused. Every position has its state.
    #[test]
    fn gemma3_windows_its_sliding_layers() {
        let sliding = gemma3_config_json(1, 2, 6);
        let (decoder, weights) = random_decoder("Gemma3ForCausalLM", &sliding);
        assert_every_position(decoder.as_ref(), &[3, 1, 4, 1, 5, 9], 16);
        let last = |decoder: &dyn Decoder, ids: &[u32]| hidden(decoder, ids).pop().unwrap();
        let prompt = [3, 1, 4, 1, 5];
        let windowed = last(decoder.as_ref(), &prompt);
        assert!(widest_gap(&windowed, &last(decoder.as_ref(), &[7, 7, 7, 1, 5])) < 1e-5);
        assert!(widest_gap(&windowed, &last(decoder.as_ref(), &[3, 1, 4, 7, 5])) > 1e-3);

        let global = gemma3_config_json(1, 2, 1);
        let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
        let decoder = gemma3_config(&global).unwrap().build(vb).unwrap();
        let whole = last(decoder.as_ref(), &prompt);
        assert!(widest_gap(&whole, &last(decoder.as_ref(), &[7, 1, 4, 1, 5])) > 1e-3);

        let scaled = |mut config: Value| -> Vec<Vec<f32>> {
            config["rope_scaling"] = json!({"rope_type": "linear", "factor": 8.0});
            assert_eq!(gemma3_config(&config).unwrap().rope_scaling_factor, 8.0);
            let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
            let decoder = gemma3_config(&config).unwrap().build(vb).unwrap();
            hidden(decoder.as_ref(), &prompt)
        };
        let (unscaled, scaled_global) = (hidden(decoder.as_ref(), &prompt), scaled(global));
        assert_eq!(unscaled[0], scaled_global[0]);
        assert!(widest_gap(&unscaled[4], &scaled_global[4]) > 1e-3);
        assert_eq!(
            scaled(sliding.clone()),
            hidden(&*random_decoder_of(&sliding, &weights), &prompt)
        );
        assert_eq!(gemma3_config(&sliding).unwrap().rope_scaling_factor, 1.0);

        let mut yarn = sliding;
        yarn["rope_scaling"] = json!({"rope_type": "yarn", "factor": 8.0});
        let err = gemma3_config(&yarn).unwrap_err();
        assert!(err.to_string().contains("yarn"), "{err:#}");
    }

    /// The decoder of a Gemma 3 `config` over `weights`.
    fn random_decoder_of(config: &Value, weights: &HashMap<String, Tensor>) -> Box<dyn Decoder> {
        let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
        gemma3_config(config).unwrap().build(vb).unwrap()
    }

    /// Final-logit softcapping in the shared readout: the slot logits of a `CausalLm` built in memory over a
    /// `final_logit_softcapping` of 30 are `30 * tanh(x / 30)` of the ones without it, and the head is the embedding
    /// (Gemma 3 ties it without a `tie_word_embeddings`). Both pinned checkpoints write `null`, which softcaps
    /// nothing.
    #[test]
    fn gemma3_softcaps_the_logits_in_the_readout() {
        let plain = gemma3_config_json(2, 4, 2);
        let (_, weights) = random_decoder("Gemma3ForCausalLM", &plain);
        let logits = |config: &Value| -> Vec<f32> {
            let configuration = gemma3_config(config).unwrap();
            assert!(configuration.shape().tied);
            let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
            let mut model = CausalLm {
                architecture: "Gemma3ForCausalLM",
                decoder: configuration.build(vb.clone()).unwrap(),
                head: vb.get((20, 16), "model.embed_tokens.weight").unwrap(),
                logits_scaling: configuration.logits_scaling(),
                softcapping: configuration.final_logit_softcapping(),
                vocab: 20,
                n_ctx: 32,
                dir: PathBuf::new(),
            };
            let logits = model.slot_logits(&[3, 1, 4, 1, 5], &[2, 4], &[0, 7, 19]);
            logits.unwrap().concat()
        };
        let raw = logits(&plain);
        assert!(raw.iter().any(|x| x.abs() > 1.0), "{raw:?}");
        let mut capped = plain;
        capped["final_logit_softcapping"] = json!(30.0);
        assert_eq!(
            gemma3_config(&capped).unwrap().final_logit_softcapping(),
            Some(30.0)
        );
        let want: Vec<f32> = raw.iter().map(|x| 30.0 * (x / 30.0).tanh()).collect();
        assert!(widest_gap(&logits(&capped), &want) < 1e-5);
    }

    /// The 883 tensor names of unsloth/gemma-3-4b-it's `model.safetensors.index.json` at the pinned commit: 34 text
    /// layers of 13 tensors with the embedding and the norm under `language_model.model.`, 27 SigLIP layers of 16
    /// with the two embeddings and the post layer norm under `vision_tower.vision_model.`, and the projector's two.
    fn gemma3_4b_weight_map() -> Vec<String> {
        let mut names = vec![
            "language_model.model.embed_tokens.weight".to_string(),
            "language_model.model.norm.weight".to_string(),
            "multi_modal_projector.mm_input_projection_weight".to_string(),
            "multi_modal_projector.mm_soft_emb_norm.weight".to_string(),
            "vision_tower.vision_model.embeddings.patch_embedding.bias".to_string(),
            "vision_tower.vision_model.embeddings.patch_embedding.weight".to_string(),
            "vision_tower.vision_model.embeddings.position_embedding.weight".to_string(),
            "vision_tower.vision_model.post_layernorm.bias".to_string(),
            "vision_tower.vision_model.post_layernorm.weight".to_string(),
        ];
        let text = [
            "input_layernorm",
            "mlp.down_proj",
            "mlp.gate_proj",
            "mlp.up_proj",
            "post_attention_layernorm",
            "post_feedforward_layernorm",
            "pre_feedforward_layernorm",
            "self_attn.k_norm",
            "self_attn.k_proj",
            "self_attn.o_proj",
            "self_attn.q_norm",
            "self_attn.q_proj",
            "self_attn.v_proj",
        ];
        for layer in 0..34 {
            for tensor in text {
                names.push(format!(
                    "language_model.model.layers.{layer}.{tensor}.weight"
                ));
            }
        }
        let vision = [
            "layer_norm1",
            "layer_norm2",
            "mlp.fc1",
            "mlp.fc2",
            "self_attn.k_proj",
            "self_attn.out_proj",
            "self_attn.q_proj",
            "self_attn.v_proj",
        ];
        for layer in 0..27 {
            for tensor in vision {
                for parameter in ["weight", "bias"] {
                    names.push(format!(
                        "vision_tower.vision_model.encoder.layers.{layer}.{tensor}.{parameter}"
                    ));
                }
            }
        }
        names
    }

    /// R6.3: a `Gemma3ForConditionalGeneration` checkpoint's configuration is its `text_config` (refused when
    /// missing), and of its weight map (883 tensors: 444 under `language_model.model.`, 437 under
    /// `vision_tower.vision_model.`, two under `multi_modal_projector.`) the text tower reads the 444 alone, each
    /// under the name a text-only checkpoint gives it. Mapped as a load maps the shards, a safetensors file holding
    /// one value per name gives up exactly those 444: no vision tensor is copied to the device.
    #[test]
    fn gemma3_multimodal_reads_the_text_tower_only() {
        let names = gemma3_4b_weight_map();
        assert_eq!(names.len(), 883);
        let under = |prefix: &str| names.iter().filter(|n| n.starts_with(prefix)).count();
        assert_eq!(under("language_model.model."), 444);
        assert_eq!(under("vision_tower.vision_model."), 437);
        assert_eq!(under("multi_modal_projector."), 2);

        let text_config = gemma3_config_json(34, 1024, 6);
        let architecture = "Gemma3ForConditionalGeneration";
        let Err(err) = configuration(architecture, &json!({"architectures": [architecture]}))
        else {
            panic!("a multimodal checkpoint without text_config read");
        };
        assert!(err.to_string().contains("text_config"), "{err:#}");
        let config = json!({"architectures": [architecture], "text_config": text_config});
        let tower = configuration(architecture, &config).unwrap();
        assert_eq!(tower.shape().max_positions, 32);
        let read: Vec<(&str, &str)> = names
            .iter()
            .filter_map(|name| tower.tensor(name).map(|read_as| (name.as_str(), read_as)))
            .collect();
        assert_eq!(read.len(), 444);
        for (name, read_as) in &read {
            assert_eq!(format!("language_model.{read_as}"), *name);
            assert!(read_as.starts_with("model."), "{read_as}");
        }

        let file = scratch("candle-gemma3-text-tower").join(snapshot::SINGLE);
        let values: HashMap<String, Tensor> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                (
                    name.clone(),
                    Tensor::new(&[i as f32], &Device::Cpu).unwrap(),
                )
            })
            .collect();
        candle_core::safetensors::save(&values, &file).unwrap();
        let tensors = read_tensors(
            std::slice::from_ref(&file),
            |name| tower.tensor(name).map(str::to_string),
            &Device::Cpu,
            DType::F32,
        )
        .unwrap();
        assert_eq!(tensors.len(), 444);
        assert!(tensors.keys().all(|name| name.starts_with("model.")));
        let embedding = tensors["model.embed_tokens.weight"]
            .to_vec1::<f32>()
            .unwrap();
        assert_eq!(embedding, [0.0]);
        let every = configuration("Gemma3ForCausalLM", &text_config).unwrap();
        let tensors = read_tensors(
            &[file],
            |name| every.tensor(name).map(str::to_string),
            &Device::Cpu,
            DType::F32,
        )
        .unwrap();
        assert_eq!(tensors.len(), 883);
    }

    /// An Olmo 3 `config.json` a few values wide: one layer per entry of `layer_types`, a window of `sliding_window`
    /// tokens and the `rope_scaling` given.
    fn olmo3_config_json(layer_types: Value, sliding_window: Value, rope_scaling: Value) -> Value {
        json!({
            "attention_bias": false,
            "hidden_act": "silu",
            "hidden_size": 16,
            "intermediate_size": 24,
            "layer_types": layer_types,
            "max_position_embeddings": 1024,
            "num_attention_heads": 2,
            "num_hidden_layers": layer_types.as_array().unwrap().len(),
            "num_key_value_heads": 2,
            "rms_norm_eps": 1e-6,
            "rope_scaling": rope_scaling,
            "rope_theta": 500000,
            "sliding_window": sliding_window,
            "tie_word_embeddings": false,
            "vocab_size": 20,
        })
    }

    /// The slot logits at every position of `ids` through a `CausalLm` built in memory over the Olmo 3 `config` and
    /// `weights`, with `head` as its output projection.
    fn olmo3_logits(
        config: &Value,
        weights: &HashMap<String, Tensor>,
        head: &Tensor,
        ids: &[u32],
    ) -> Vec<Vec<f32>> {
        let configuration = olmo3_config(config).unwrap();
        let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
        let mut model = CausalLm {
            architecture: "Olmo3ForCausalLM",
            decoder: configuration.build(vb).unwrap(),
            head: head.clone(),
            logits_scaling: configuration.logits_scaling(),
            softcapping: None,
            vocab: 20,
            n_ctx: 1024,
            dir: PathBuf::new(),
        };
        let slots: Vec<usize> = (0..ids.len()).collect();
        model.slot_logits(ids, &slots, &[0, 7, 19]).unwrap()
    }

    /// R7.3, the vendored Olmo 3 over random weights: two layers, `layer_types` `[sliding_attention,
    /// full_attention]` and a `sliding_window` of 8. A 16-token prompt's slot logits change when the window is set to
    /// 16 (every key in reach: full attention) at the positions from 8 on, which lose a key under the window of 8,
    /// and stay the same before them; an 8-token prompt gives identical logits under both windows. Every position
    /// has its state, past the 512 query rows one attention matmul covers too. A sliding layer without a window and a
    /// layer of another type are refused.
    #[test]
    fn olmo3_sliding_layers_see_their_window() {
        let types = json!(["sliding_attention", "full_attention"]);
        let narrow = olmo3_config_json(types.clone(), json!(8), Value::Null);
        let (decoder, weights) = random_decoder("Olmo3ForCausalLM", &narrow);
        assert_every_position(decoder.as_ref(), &[3, 1, 4, 1, 5, 9, 2, 6, 5, 3], 16);
        // Past 512 tokens the attention runs in chunks of query rows: 513 tokens (two chunks) have the states their
        // first 512 (one chunk) have.
        let two_chunks: Vec<u32> = (0..513).map(|i| (i * 7 + 3) % 20).collect();
        assert_every_position(decoder.as_ref(), &two_chunks, 16);

        let head = Tensor::randn(0f32, 1., (20, 16), &Device::Cpu).unwrap();
        let wide = olmo3_config_json(types.clone(), json!(16), Value::Null);
        let sixteen: Vec<u32> = (0..16).map(|i| (i * 7 + 3) % 20).collect();
        let (under_8, under_16) = (
            olmo3_logits(&narrow, &weights, &head, &sixteen),
            olmo3_logits(&wide, &weights, &head, &sixteen),
        );
        for (position, (a, b)) in under_8.iter().zip(&under_16).enumerate() {
            let gap = widest_gap(a, b);
            if position < 8 {
                assert!(gap < 1e-6, "position {position}: the logits moved by {gap}");
            } else {
                assert!(gap > 1e-3, "position {position}: the logits moved by {gap}");
            }
        }
        let eight = &sixteen[..8];
        assert_eq!(
            olmo3_logits(&narrow, &weights, &head, eight),
            olmo3_logits(&wide, &weights, &head, eight)
        );

        let err = olmo3_config(&olmo3_config_json(types, Value::Null, Value::Null)).unwrap_err();
        assert!(err.to_string().contains("sliding_window"), "{err:#}");
        let conv = json!(["conv", "full_attention"]);
        let err = olmo3_config(&olmo3_config_json(conv, json!(8), Value::Null)).unwrap_err();
        assert!(err.to_string().contains("conv"), "{err:#}");
        let mut none = narrow;
        none.as_object_mut().unwrap().remove("layer_types");
        let err = olmo3_config(&none).unwrap_err();
        assert!(err.to_string().contains("layer_types"), "{err:#}");
    }

    /// The vendored Olmo 3's YaRN, as transformers computes it for Olmo-3-7B-Instruct's own parameters (heads 128
    /// wide, `rope_theta` 500,000, `original_max_position_embeddings` 8192, `factor` 8, `beta_fast` 32, `beta_slow`
    /// 1): the correction range runs from pair 18 (`floor(128 ln(8192 / 64 pi) / 2 ln 500000)` = 18.08) to pair 35
    /// (`ceil` of 34.98 for one rotation), so the pairs up to 18 keep their frequency, those from 35 on take it
    /// divided by 8, and pair 26 mixes the two by 8/17; `attention_factor` is `0.1 ln 8 + 1` when left out, the
    /// betas 32 and 1. Over random weights the scaling moves the full-attention layers alone, at every position but
    /// the first, which attends to itself alone whatever its scores; another `rope_type` is refused.
    #[test]
    fn olmo3_scales_the_full_layers_by_yarn() {
        let yarn = olmo2::Yarn {
            factor: 8.0,
            original_max_position_embeddings: 8192,
            attention_factor: 1.2079441541679836,
            beta_fast: 32.0,
            beta_slow: 1.0,
        };
        let inv_freq = yarn.inv_freq(128, 500_000.0);
        assert_eq!(inv_freq.len(), 64);
        let unscaled = |i: usize| 500_000f64.powf(-(i as f64) / 64.0);
        for (i, got) in inv_freq.iter().enumerate() {
            let want = match i {
                0..=18 => unscaled(i),
                35.. => unscaled(i) / 8.0,
                26 => unscaled(i) * (9.0 / 17.0 + 8.0 / 17.0 / 8.0),
                _ => continue,
            };
            let gap = (f64::from(*got) - want).abs() / want;
            assert!(gap < 1e-6, "pair {i}: {got}, not {want}");
        }

        let scaling = json!({
            "rope_type": "yarn",
            "factor": 8.0,
            "original_max_position_embeddings": 8192,
        });
        let full = olmo3_config_json(
            json!(["full_attention", "full_attention"]),
            json!(8),
            scaling.clone(),
        );
        assert_eq!(olmo3_config(&full).unwrap().rope_scaling, Some(yarn));
        let (decoder, weights) = random_decoder("Olmo3ForCausalLM", &full);
        let prompt = [3, 1, 4, 1, 5];
        assert_every_position(decoder.as_ref(), &prompt, 16);
        let plain = |config: &Value| {
            let mut config = config.clone();
            config["rope_scaling"] = Value::Null;
            config
        };
        let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
        let unscaled = olmo3_config(&plain(&full)).unwrap().build(vb).unwrap();
        let (scaled, unscaled) = (
            hidden(decoder.as_ref(), &prompt),
            hidden(unscaled.as_ref(), &prompt),
        );
        assert_eq!(scaled[0], unscaled[0]);
        for (position, (a, b)) in scaled.iter().zip(&unscaled).enumerate().skip(1) {
            assert!(
                widest_gap(a, b) > 1e-3,
                "position {position}: {a:?} and {b:?}"
            );
        }

        let sliding = olmo3_config_json(
            json!(["sliding_attention", "sliding_attention"]),
            json!(8),
            scaling,
        );
        let build = |config: &Value| {
            let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
            olmo3_config(config).unwrap().build(vb).unwrap()
        };
        assert_eq!(
            hidden(&*build(&sliding), &prompt),
            hidden(&*build(&plain(&sliding)), &prompt)
        );

        let mut linear = sliding;
        linear["rope_scaling"] = json!({"rope_type": "linear", "factor": 8.0});
        let err = olmo3_config(&linear).unwrap_err();
        assert!(err.to_string().contains("linear"), "{err:#}");
    }

    /// Q5: `gpu_layers` 0 is the CPU in F32 and below -1 an error; -1 is the CPU too on a build without Metal (an Apple
    /// silicon build opens Metal there, which the real-model tests load on).
    #[test]
    fn gpu_layers_pick_the_device() {
        let (cpu, dtype) = device(0).unwrap();
        assert!(cpu.is_cpu());
        assert_eq!(dtype, DType::F32);
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            let (all, dtype) = device(-1).unwrap();
            assert!(all.is_cpu());
            assert_eq!(dtype, DType::F32);
        }
        assert!(device(-2).is_err());
    }

    /// A THX-01 `encoder/config.json` a few values wide, in mmBERT's shape: the rotary thetas under
    /// `rope_parameters`, `layer_types` agreeing with `global_attn_every_n_layers` (2), every bias off.
    fn thx01_encoder_json() -> Value {
        json!({
            "architectures": ["ModernBertForMaskedLM"],
            "vocab_size": 20,
            "hidden_size": 16,
            "num_hidden_layers": 2,
            "num_attention_heads": 2,
            "intermediate_size": 8,
            "max_position_embeddings": 64,
            "norm_eps": 1e-5,
            "layer_norm_eps": 1e-5,
            "pad_token_id": 0,
            "global_attn_every_n_layers": 2,
            "local_attention": 4,
            "norm_bias": false,
            "attention_bias": false,
            "mlp_bias": false,
            "layer_types": ["full_attention", "sliding_attention"],
            "rope_parameters": {
                "full_attention": {"rope_theta": 160000, "rope_type": "default"},
                "sliding_attention": {"rope_theta": 10000, "rope_type": "default"},
            },
        })
    }

    /// An `rl_agent_config.json` with THX-01's base triple and two buckets, one below the clamp.
    fn thx01_decision_json() -> Value {
        json!({
            "head_layers": 2,
            "max_len": 64,
            "head_max_len": 16,
            "tournament_above": 24,
            "temperature": [2.55, 3.05, 2.0],
            "temperature_by_options": {"noul:2": 5.0, "choice:3-5": 0.1},
        })
    }

    /// `encoder/config.json` as candle's ModernBERT configuration: the nested thetas, else the flat ones; a bias on, or
    /// `layer_types` that disagree with `global_attn_every_n_layers`, is refused. `rl_agent_config.json`'s
    /// temperatures: a bucket's, clamped to [0.5, 5.0], else the type's base one.
    #[test]
    fn thx01_reads_its_configs() {
        let encoder = thx01::encoder_config(&thx01_encoder_json()).unwrap();
        assert_eq!(
            (encoder.global_rope_theta, encoder.local_rope_theta),
            (160000.0, 10000.0)
        );
        assert_eq!((encoder.hidden_size, encoder.local_attention), (16, 4));
        let mut flat = thx01_encoder_json();
        flat.as_object_mut().unwrap().remove("rope_parameters");
        flat["global_rope_theta"] = json!(1000.0);
        flat["local_rope_theta"] = json!(100.0);
        let flat = thx01::encoder_config(&flat).unwrap();
        assert_eq!(
            (flat.global_rope_theta, flat.local_rope_theta),
            (1000.0, 100.0)
        );
        let mut biased = thx01_encoder_json();
        biased["mlp_bias"] = json!(true);
        let err = thx01::encoder_config(&biased).unwrap_err();
        assert!(err.to_string().contains("mlp_bias"), "{err:#}");
        let mut swapped = thx01_encoder_json();
        swapped["layer_types"] = json!(["sliding_attention", "full_attention"]);
        let err = thx01::encoder_config(&swapped).unwrap_err();
        assert!(err.to_string().contains("layer_types[0]"), "{err:#}");

        let config = thx01::Config::new(encoder, &thx01_decision_json()).unwrap();
        assert_eq!(config.head_layers, 2);
        let t = &config.temperatures;
        assert_eq!(t.of(ardana_core::AnswerType::Noul, 2), 5.0);
        assert_eq!(t.of(ardana_core::AnswerType::Choice, 3), 0.5);
        assert_eq!(t.of(ardana_core::AnswerType::Choice, 5), 0.5);
        assert_eq!(t.of(ardana_core::AnswerType::Choice, 11), 2.55);
        assert_eq!(t.of(ardana_core::AnswerType::Score, 4), 3.05);
        assert_eq!(t.of(ardana_core::AnswerType::Noul, 3), 2.0);

        assert_eq!(
            thx01::tensor("encoder.layers.0.attn.Wqkv.weight").as_deref(),
            Some("model.layers.0.attn.Wqkv.weight")
        );
        assert_eq!(
            thx01::tensor("head.layers.1.self_attn.in_proj_weight").as_deref(),
            Some("head.layers.1.self_attn.in_proj_weight")
        );
        assert_eq!(
            thx01::tensor("scorer.3.bias").as_deref(),
            Some("scorer.3.bias")
        );
        assert_eq!(
            thx01::tensor("type_emb.weight").as_deref(),
            Some("type_emb.weight")
        );
        assert_eq!(thx01::tensor("act_head.0.weight"), None);
        assert_eq!(thx01::tensor("temperature"), None);
    }

    /// A sequence of `len` tokens with markers at `markers`, of `kind`.
    fn sequence(
        len: usize,
        markers: &[usize],
        kind: ardana_core::AnswerType,
    ) -> ardana_core::DecisionSequence {
        ardana_core::DecisionSequence {
            ids: (0..len).map(|i| (i * 7 + 3) as u32 % 20).collect(),
            markers: markers.to_vec(),
            kind,
        }
    }

    /// R8.5, R8.6: THX-01 over random weights, saved as a checkpoint (`config.json` naming `THXDecisionModel`,
    /// `encoder/config.json`, `rl_agent_config.json`, the tensors under the checkpoint's names with the action head
    /// and the temperature buffer beside them): `CandleRuntime` supports the directory and loads it as a
    /// `Loaded::Decision`; the model's tensors are F32 at `gpu_layers` 0 and at the default -1 alike; every sequence
    /// gets one probability per marker, summing to 1, the same from the saved checkpoint as from the model in memory
    /// and the same on a second call; the bucket temperature flattens the answer (`noul:2` at 5.0 against 0.5); an
    /// empty sequence, a marker past the end and a sequence past the encoder's positions are errors.
    #[test]
    fn thx01_computes_in_f32() {
        use ardana_core::AnswerType::{Choice, Noul};
        use ardana_core::DecisionModel;

        let decision = thx01_decision_json();
        let config = thx01::Config::new(
            thx01::encoder_config(&thx01_encoder_json()).unwrap(),
            &decision,
        )
        .unwrap();
        let mut varmap = candle_nn::VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &Device::Cpu);
        let mut in_memory = thx01::Thx01::build(&config, vb).unwrap();
        let mut weights: HashMap<String, Tensor> = {
            let data = varmap.data().lock().unwrap();
            data.iter()
                .map(|(name, var)| (name.clone(), var.as_tensor().clone()))
                .collect()
        };
        // A `VarMap` starts the type embedding at zero: give it values, in memory and in the file alike.
        let type_emb = Tensor::randn(0f32, 1., (3, 16), &Device::Cpu).unwrap();
        varmap.set_one("type_emb.weight", &type_emb).unwrap();
        weights.insert("type_emb.weight".into(), type_emb);

        let dir = scratch("candle-thx01");
        std::fs::create_dir_all(dir.join("encoder")).unwrap();
        std::fs::write(
            dir.join(snapshot::CONFIG),
            json!({"architectures": ["THXDecisionModel"], "model_type": "thx01"}).to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join(ardana_core::thx01::ENCODER_CONFIG),
            thx01_encoder_json().to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join(ardana_core::thx01::DECISION_CONFIG),
            decision.to_string(),
        )
        .unwrap();
        let mut saved: HashMap<String, Tensor> = weights
            .iter()
            .map(|(name, tensor)| {
                let name = match name.strip_prefix("model.") {
                    Some(rest) => format!("encoder.{rest}"),
                    None => name.clone(),
                };
                (name, tensor.clone())
            })
            .collect();
        saved.insert(
            "act_head.0.weight".into(),
            Tensor::zeros((256, 20), DType::F32, &Device::Cpu).unwrap(),
        );
        saved.insert(
            "temperature".into(),
            Tensor::ones(3, DType::F32, &Device::Cpu).unwrap(),
        );
        candle_core::safetensors::save(&saved, dir.join(snapshot::SINGLE)).unwrap();
        for (name, tensor) in &saved {
            assert_eq!(
                thx01::tensor(name).is_some(),
                !name.starts_with("act_head.") && name != "temperature",
                "{name}"
            );
            assert_eq!(tensor.dtype(), DType::F32);
        }

        let runtime = CandleRuntime;
        assert!(runtime.supports(&dir));
        let sequences = [
            sequence(12, &[3, 6, 9], Choice),
            sequence(9, &[2, 5], Noul),
            sequence(12, &[3, 6, 9], Choice),
        ];
        let expected = in_memory.decide(&sequences).unwrap();
        assert_eq!(expected[0], expected[2]);
        for (probs, seq) in expected.iter().zip(&sequences) {
            assert_eq!(probs.len(), seq.markers.len());
            assert!((probs.iter().sum::<f64>() - 1.0).abs() < 1e-6, "{probs:?}");
        }
        for gpu_layers in [0, -1] {
            let opts = LoadOptions {
                gpu_layers,
                ..LoadOptions::default()
            };
            let mut loaded = match runtime.load(&dir, &opts).unwrap() {
                Loaded::Decision(model) => model,
                Loaded::Causal(_) => panic!("THX-01 loaded as a causal model"),
            };
            let model = thx01::Thx01::load(&dir, &device(gpu_layers).unwrap().0).unwrap();
            assert_eq!(model.dtype(), DType::F32);
            assert_eq!(thx01::DTYPE, DType::F32);
            let got = loaded.decide(&sequences).unwrap();
            for (got, want) in got.iter().zip(&expected) {
                assert_eq!(got.len(), want.len());
                assert!(widest_gap_f64(got, want) < 1e-5, "{got:?} and {want:?}");
            }
            assert_eq!(loaded.decide(&sequences).unwrap(), got);
        }

        // The noul bucket at 5.0 flattens the answer the same weights give at 0.5.
        let mut sharp = decision.clone();
        sharp["temperature_by_options"]["noul:2"] = json!(0.5);
        let sharp = thx01::Config::new(
            thx01::encoder_config(&thx01_encoder_json()).unwrap(),
            &sharp,
        )
        .unwrap();
        let vb = VarBuilder::from_tensors(weights.clone(), DType::F32, &Device::Cpu);
        let mut sharp = thx01::Thx01::build(&sharp, vb).unwrap();
        let noul = &sequences[1..2];
        let (flat, peaked) = (
            in_memory.decide(noul).unwrap().remove(0),
            sharp.decide(noul).unwrap().remove(0),
        );
        let top = |p: &[f64]| p.iter().copied().fold(0.0, f64::max);
        assert!(top(&flat) < top(&peaked), "{flat:?} against {peaked:?}");
        assert!(top(&peaked) > 0.5 + 1e-9, "{peaked:?}");

        assert!(in_memory.decide(&[sequence(0, &[], Choice)]).is_err());
        assert!(in_memory.decide(&[sequence(12, &[12], Choice)]).is_err());
        assert!(in_memory.decide(&[sequence(12, &[], Choice)]).is_err());
        assert!(in_memory.decide(&[sequence(65, &[1, 2], Choice)]).is_err());
        assert!(in_memory.decide(&[sequence(64, &[1, 2], Choice)]).is_ok());
    }

    /// The widest gap between two probability vectors.
    fn widest_gap_f64(a: &[f64], b: &[f64]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(a, b)| (a - b).abs())
            .fold(0f64, f64::max)
    }

    /// Q11 and the one-prompt contract on the real checkpoint: the window is the smaller of the option and
    /// `max_position_embeddings`; a prompt decodes alone (the same prompt answers the same after another), and an
    /// empty prompt, a slot past the prompt, a label past the vocabulary and a prompt past the window are errors.
    #[test]
    #[ignore = "e2e: Qwen3-1.7B safetensors in tmp/hf (cargo xtask fetch), loaded through Candle"]
    fn qwen3_decodes_each_prompt_alone() {
        let dir = qwen3_snapshot();
        let load = |n_ctx| {
            CandleRuntime
                .load(
                    &dir,
                    &LoadOptions {
                        n_ctx,
                        gpu_layers: -1,
                    },
                )
                .unwrap()
        };
        let load = |n_ctx| match load(n_ctx) {
            Loaded::Causal(model) => model,
            Loaded::Decision(_) => panic!("Qwen3 loaded as a decision model"),
        };
        assert_eq!(load(100_000).n_ctx(), 40_960);
        let mut model = load(64);
        assert_eq!(model.n_ctx(), 64);
        // "The capital of France is" and an unrelated prompt; the labels " Paris", " London", " the".
        let first: Vec<u32> = vec![785, 6722, 315, 9625, 374];
        let second: Vec<u32> = vec![40, 1075, 311, 8180, 304, 279, 13638, 13];
        let labels = [12095, 7148, 279];
        let before = model.slot_logits(&first, &[2, 4], &labels).unwrap();
        assert_eq!((before.len(), before[1].len()), (2, 3));
        assert!(before[1][0] > before[1][1], "{before:?}");
        model.slot_logits(&second, &[7], &labels).unwrap();
        let after = model.slot_logits(&first, &[2, 4], &labels).unwrap();
        assert_eq!(before, after);

        assert!(model.slot_logits(&[], &[], &labels).is_err());
        assert!(model.slot_logits(&first, &[5], &labels).is_err());
        assert!(model.slot_logits(&first, &[4], &[151_936]).is_err());
        assert!(model.slot_logits(&[1; 65], &[64], &labels).is_err());
    }
}
