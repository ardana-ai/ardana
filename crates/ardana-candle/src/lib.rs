//! The Candle runtime behind `ardana-core`'s runtime traits: safetensors causal LMs read from a Hugging Face snapshot
//! directory ([`ardana_core::snapshot`]), each architecture `config.json#architectures` names mapped to one
//! implementation ([`ARCHITECTURES`]), never chosen by a model name.
//!
//! A model computes on the CPU in F32; an Apple silicon build puts it on Metal in BF16 unless `gpu_layers` is 0. An
//! architecture is its configuration ([`Architecture`]) and its decoder ([`Decoder`]), which returns the hidden states
//! at every position; the readout is shared ([`CausalLm`]): the label logits are the head's rows for the label ids
//! applied to the hidden states at the slots, in F32, so a prompt never computes a whole vocabulary row.
//!
//! candle-transformers' Llama and Phi-3 narrow to the last position inside the model, so [`llama`] and [`phi3`] are
//! its 0.11.0 files, copied and changed as each one's header lists.

mod llama;
mod phi3;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use ardana_core::runtime::{LoadOptions, LoadedModel, Runtime};
use ardana_core::snapshot;
use candle_core::safetensors::MmapedSafetensors;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::smol::smollm3;
use candle_transformers::models::{qwen2, qwen3};
use serde_json::Value;

/// The `config.json#architectures` names this runtime implements.
pub const ARCHITECTURES: [&str; 5] = [
    "Qwen3ForCausalLM",
    "Qwen2ForCausalLM",
    "SmolLM3ForCausalLM",
    "LlamaForCausalLM",
    "Phi3ForCausalLM",
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

    fn load(&self, weights: &Path, opts: &LoadOptions) -> Result<Box<dyn LoadedModel>> {
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
        let model = CausalLm::load(weights, architecture, &config, opts, &device, dtype)?;
        Ok(Box::new(model))
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

/// Every tensor of `files` on `device` in `dtype`, each converted once, so a tied head shares its embedding's storage.
fn read_tensors(files: &[PathBuf], device: &Device, dtype: DType) -> Result<VarBuilder<'static>> {
    // SAFETY: the files are hub cache blobs, which are never written in place once downloaded
    // (docs/guidelines/huggingface.md, "Hub cache layout"), so the mapping does not change while it is read; every
    // tensor is copied out of it before it is dropped at the end of this function.
    let mapped = unsafe { MmapedSafetensors::multi(files) }
        .with_context(|| format!("mapping {}", display(files)))?;
    let mut tensors = HashMap::new();
    for (name, _) in mapped.tensors() {
        let tensor = mapped
            .load(&name, device)
            .and_then(|tensor| tensor.to_dtype(dtype))
            .with_context(|| format!("reading the tensor {name}"))?;
        tensors.insert(name, tensor);
    }
    Ok(VarBuilder::from_tensors(tensors, dtype, device))
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

/// A loaded causal LM of any architecture: its decoder and the readout every architecture shares.
pub struct CausalLm {
    architecture: &'static str,
    decoder: Box<dyn Decoder>,
    /// The output projection, `[vocab, hidden]`: `lm_head.weight`, or the embedding it is tied to.
    head: Tensor,
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
        let vb = read_tensors(&files, device, dtype)?;
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
        let logits = at_slots.matmul(&labels.t()?)?;
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
                 LlamaForCausalLM, Phi3ForCausalLM"
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
