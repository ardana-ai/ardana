//! THX-01 (`config.json#architectures` `THXDecisionModel`), the `thx01` 1.0.0 package's `DecisionModel`
//! (`thx01/common.py`) as a [`DecisionModel`] of `ardana-core`: candle-transformers' ModernBERT over the `encoder.*`
//! tensors, the question type's embedding added to every position, two PyTorch `TransformerEncoderLayer`s
//! (`norm_first`, ReLU, biases) and the scorer (LayerNorm, Linear, GELU, Linear) read at the sequence's markers, one
//! logit per option. The softmax over the logits divided by the bucket temperature of `rl_agent_config.json`
//! (`temperature_by_options["<type>:<bucket>"]`, else `temperature[<type>]`, each clamped to [0.5, 5.0] as the package
//! clamps it) is the probability vector the engine assembles. Everything computes in F32, on the CPU or on Metal: the
//! encoder's masks are F32 tensors added to the scores.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use ardana_core::engine::{DecisionSequence, softmax};
use ardana_core::profile::{AnswerType, clamp_temperature};
use ardana_core::runtime::DecisionModel;
use ardana_core::snapshot;
use ardana_core::thx01::{DECISION_CONFIG, ENCODER_CONFIG};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{LayerNorm, Linear, VarBuilder, layer_norm, linear};
use candle_transformers::models::modernbert::{self, ModernBert};
use serde_json::Value;

/// The architecture name a THX-01 `config.json` writes.
pub const ARCHITECTURE: &str = "THXDecisionModel";
/// The precision every THX-01 tensor is read in and computed in, whatever the device.
pub const DTYPE: DType = DType::F32;
/// PyTorch's `LayerNorm` default, which the head layers and the scorer use.
const LAYER_NORM_EPS: f64 = 1e-5;
/// The package's `temperature` triple is in its `QTYPES` order: choice, score, noul.
const TYPES: [AnswerType; 3] = [AnswerType::Choice, AnswerType::Score, AnswerType::Noul];

/// What `encoder/config.json` and `rl_agent_config.json` give a load, read before any tensor.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub encoder: modernbert::Config,
    /// `head_layers`: the `TransformerEncoderLayer`s over the encoder's states (2).
    pub head_layers: usize,
    pub temperatures: Temperatures,
}

/// The temperatures `Agent._decode` divides the logits by: the per-bucket ones, else the type's base one.
#[derive(Debug, Clone, PartialEq)]
pub struct Temperatures {
    /// `temperature`, clamped: choice, score, noul.
    base: [f64; 3],
    /// `temperature_by_options`, clamped, by `<type>:<bucket>` (`noul:2`, `choice:3-5`, `choice:6-10`, `score:3-5`,
    /// `choice:11+`).
    by_bucket: HashMap<String, f64>,
}

impl Temperatures {
    /// `temp_bucket`: the temperature of a question of `kind` with `options` options.
    pub fn of(&self, kind: AnswerType, options: usize) -> f64 {
        let size = match options {
            ..=2 => "2",
            3..=5 => "3-5",
            6..=10 => "6-10",
            _ => "11+",
        };
        self.by_bucket
            .get(&format!("{kind}:{size}"))
            .copied()
            .unwrap_or(self.base[qtype(kind)])
    }
}

/// `QTYPES`: the row of `type_emb` and of `temperature` a question type reads.
fn qtype(kind: AnswerType) -> usize {
    match kind {
        AnswerType::Choice => 0,
        AnswerType::Score => 1,
        AnswerType::Noul => 2,
    }
}

impl Config {
    /// The configuration of the checkpoint in `dir`: `encoder/config.json` and `rl_agent_config.json`.
    pub fn read(dir: &Path) -> Result<Config> {
        let read = |file: &str| -> Result<Value> {
            let path = dir.join(file);
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        };
        let encoder = encoder_config(&read(ENCODER_CONFIG)?).context(ENCODER_CONFIG)?;
        let decision = read(DECISION_CONFIG)?;
        Config::new(encoder, &decision).context(DECISION_CONFIG)
    }

    /// The configuration over the ModernBERT `encoder` and the `rl_agent_config.json` value `decision`.
    pub fn new(encoder: modernbert::Config, decision: &Value) -> Result<Config> {
        let head_layers = match &decision["head_layers"] {
            Value::Null => 2,
            Value::Number(n) => n
                .as_u64()
                .with_context(|| format!("head_layers {n} is no count"))?
                as usize,
            other => bail!("head_layers {other} is no count"),
        };
        let base = match &decision["temperature"] {
            Value::Null => [1.0; 3],
            Value::Array(items) if items.len() == 3 => {
                let mut base = [1.0; 3];
                for (slot, item) in base.iter_mut().zip(items) {
                    *slot = clamp_temperature(item);
                }
                base
            }
            other => bail!("temperature {other} is no list of three (choice, score, noul)"),
        };
        let by_bucket = match &decision["temperature_by_options"] {
            Value::Null => HashMap::new(),
            Value::Object(map) => map
                .iter()
                .map(|(bucket, t)| (bucket.clone(), clamp_temperature(t)))
                .collect(),
            other => bail!("temperature_by_options {other} is no map"),
        };
        Ok(Config {
            encoder,
            head_layers,
            temperatures: Temperatures { base, by_bucket },
        })
    }
}

/// `encoder/config.json` (mmBERT-base, `ModernBertForMaskedLM`) as candle's ModernBERT configuration: candle's own
/// fields, the rotary thetas from `rope_parameters.{full_attention,sliding_attention}.rope_theta` (transformers 5)
/// or the flat `global_rope_theta` and `local_rope_theta` (transformers 4), and the norm epsilon from `norm_eps`,
/// else `layer_norm_eps`. candle's layers have no biases and alternate by `global_attn_every_n_layers`, so a config
/// with `norm_bias`, `attention_bias` or `mlp_bias` on, or whose `layer_types` disagree with that count, is refused.
pub fn encoder_config(config: &Value) -> Result<modernbert::Config> {
    let count = |name: &str| -> Result<usize> {
        serde_json::from_value(config[name].clone()).context(name.to_string())
    };
    for bias in ["norm_bias", "attention_bias", "mlp_bias"] {
        ensure!(
            config[bias] != Value::Bool(true),
            "{bias} is true, and candle's ModernBERT reads no such bias"
        );
    }
    let theta = |layer: &str, flat: &str| -> Result<f64> {
        let nested = &config["rope_parameters"][layer]["rope_theta"];
        let value = if nested.is_null() {
            &config[flat]
        } else {
            nested
        };
        serde_json::from_value(value.clone())
            .with_context(|| format!("rope_parameters.{layer}.rope_theta or {flat}"))
    };
    let eps = if config["norm_eps"].is_null() {
        "layer_norm_eps"
    } else {
        "norm_eps"
    };
    let global_attn_every_n_layers = count("global_attn_every_n_layers")?;
    ensure!(
        global_attn_every_n_layers > 0,
        "global_attn_every_n_layers must be at least 1"
    );
    if let Value::Array(layer_types) = &config["layer_types"] {
        for (layer, layer_type) in layer_types.iter().enumerate() {
            let global = layer % global_attn_every_n_layers == 0;
            let want = if global {
                "full_attention"
            } else {
                "sliding_attention"
            };
            ensure!(
                layer_type == want,
                "layer_types[{layer}] is {layer_type}, and candle alternates layers by global_attn_every_n_layers \
                 ({global_attn_every_n_layers}), which makes it {want}"
            );
        }
    }
    Ok(modernbert::Config {
        vocab_size: count("vocab_size")?,
        hidden_size: count("hidden_size")?,
        num_hidden_layers: count("num_hidden_layers")?,
        num_attention_heads: count("num_attention_heads")?,
        intermediate_size: count("intermediate_size")?,
        max_position_embeddings: count("max_position_embeddings")?,
        layer_norm_eps: serde_json::from_value(config[eps].clone()).context(eps.to_string())?,
        pad_token_id: serde_json::from_value(config["pad_token_id"].clone())
            .context("pad_token_id")?,
        global_attn_every_n_layers,
        global_rope_theta: theta("full_attention", "global_rope_theta")?,
        local_attention: count("local_attention")?,
        local_rope_theta: theta("sliding_attention", "local_rope_theta")?,
        classifier_config: None,
    })
}

/// The name a THX-01 checkpoint tensor is read under: the encoder's under `model.`, where candle's ModernBERT reads
/// them; the head's, the scorer's and the type embedding under their own; `act_head.*` (the action head, no answer) and
/// `temperature` (a buffer the package never applies) not at all.
pub fn tensor(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_prefix("encoder.") {
        return Some(format!("model.{rest}"));
    }
    ["head.", "scorer.", "type_emb."]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        .then(|| name.to_string())
}

/// One `nn.TransformerEncoderLayer(d, d / 64, 4d, dropout, batch_first=True, norm_first=True)`: `x + sa(norm1(x))`,
/// then `x + linear2(relu(linear1(norm2(x))))`, the self-attention PyTorch's `MultiheadAttention` with its packed
/// `in_proj` and its `out_proj`, over one sequence (no padding mask).
struct HeadLayer {
    norm1: LayerNorm,
    in_proj: Linear,
    out_proj: Linear,
    norm2: LayerNorm,
    linear1: Linear,
    linear2: Linear,
    heads: usize,
}

impl HeadLayer {
    fn load(vb: VarBuilder, hidden: usize, heads: usize) -> candle_core::Result<HeadLayer> {
        let attn = vb.pp("self_attn");
        let in_proj = Linear::new(
            attn.get_with_hints(
                (3 * hidden, hidden),
                "in_proj_weight",
                candle_nn::init::DEFAULT_KAIMING_NORMAL,
            )?,
            Some(attn.get(3 * hidden, "in_proj_bias")?),
        );
        Ok(HeadLayer {
            norm1: layer_norm(hidden, LAYER_NORM_EPS, vb.pp("norm1"))?,
            in_proj,
            out_proj: linear(hidden, hidden, attn.pp("out_proj"))?,
            norm2: layer_norm(hidden, LAYER_NORM_EPS, vb.pp("norm2"))?,
            linear1: linear(hidden, 4 * hidden, vb.pp("linear1"))?,
            linear2: linear(4 * hidden, hidden, vb.pp("linear2"))?,
            heads,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let (b, len, d) = xs.dims3()?;
        let head_dim = d / self.heads;
        let qkv = xs
            .apply(&self.norm1)?
            .apply(&self.in_proj)?
            .reshape((b, len, 3, self.heads, head_dim))?
            .permute((2, 0, 3, 1, 4))?;
        let (q, k, v) = (
            qkv.get(0)?.contiguous()?,
            qkv.get(1)?.contiguous()?,
            qkv.get(2)?.contiguous()?,
        );
        let scores = (q.matmul(&k.transpose(D::Minus2, D::Minus1)?.contiguous()?)?
            * (head_dim as f64).powf(-0.5))?;
        let attended = candle_nn::ops::softmax(&scores, D::Minus1)?
            .matmul(&v)?
            .transpose(1, 2)?
            .reshape((b, len, d))?
            .apply(&self.out_proj)?;
        let xs = (xs + attended)?;
        let ff = xs
            .apply(&self.norm2)?
            .apply(&self.linear1)?
            .relu()?
            .apply(&self.linear2)?;
        xs + ff
    }
}

/// `nn.Sequential(nn.LayerNorm(d), nn.Linear(d, d), nn.GELU(), nn.Linear(d, 1))`: one logit per state.
struct Scorer {
    norm: LayerNorm,
    dense: Linear,
    out: Linear,
}

impl Scorer {
    fn load(vb: VarBuilder, hidden: usize) -> candle_core::Result<Scorer> {
        Ok(Scorer {
            norm: layer_norm(hidden, LAYER_NORM_EPS, vb.pp("0"))?,
            dense: linear(hidden, hidden, vb.pp("1"))?,
            out: linear(hidden, 1, vb.pp("3"))?,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        xs.apply(&self.norm)?
            .apply(&self.dense)?
            .gelu_erf()?
            .apply(&self.out)
    }
}

/// A loaded THX-01.
pub struct Thx01 {
    encoder: ModernBert,
    /// `type_emb.weight`, `[3, hidden]`: the row of a question type is added to every position.
    type_emb: Tensor,
    head: Vec<HeadLayer>,
    scorer: Scorer,
    temperatures: Temperatures,
    /// `max_position_embeddings`: where the encoder's rotary tables end.
    max_positions: usize,
}

impl std::fmt::Debug for Thx01 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thx01")
            .field("head_layers", &self.head.len())
            .field("max_positions", &self.max_positions)
            .field("device", self.type_emb.device())
            .finish()
    }
}

impl Thx01 {
    /// The model over the checkpoint's tensors, read by name through [`tensor`], on `vb`'s device in [`DTYPE`].
    pub fn build(config: &Config, vb: VarBuilder<'static>) -> candle_core::Result<Thx01> {
        let hidden = config.encoder.hidden_size;
        let encoder = ModernBert::load(vb.clone(), &config.encoder)?;
        let type_emb = vb.get((TYPES.len(), hidden), "type_emb.weight")?;
        // `nhead = max(1, d // 64)`.
        let heads = (hidden / 64).max(1);
        let head = (0..config.head_layers)
            .map(|layer| HeadLayer::load(vb.pp(format!("head.layers.{layer}")), hidden, heads))
            .collect::<candle_core::Result<Vec<_>>>()?;
        let scorer = Scorer::load(vb.pp("scorer"), hidden)?;
        Ok(Thx01 {
            encoder,
            type_emb,
            head,
            scorer,
            temperatures: config.temperatures.clone(),
            max_positions: config.encoder.max_position_embeddings,
        })
    }

    /// The model from the snapshot directory `dir`: its configuration, then every tensor [`tensor`] names from its
    /// weight files, converted once to [`DTYPE`] on `device`.
    pub fn load(dir: &Path, device: &Device) -> Result<Thx01> {
        let config = Config::read(dir)?;
        let files = snapshot::weight_files(dir).map_err(anyhow::Error::msg)?;
        let tensors = crate::read_tensors(&files, tensor, device, DTYPE)?;
        let vb = VarBuilder::from_tensors(tensors, DTYPE, device);
        Thx01::build(&config, vb)
            .with_context(|| format!("building {ARCHITECTURE} from {}", dir.display()))
    }

    /// The precision of the loaded tensors ([`DTYPE`]).
    pub fn dtype(&self) -> DType {
        self.type_emb.dtype()
    }

    /// `DecisionModel.forward` for one sequence: the logits at its markers.
    fn logits(&self, seq: &DecisionSequence) -> Result<Vec<f32>> {
        let len = seq.ids.len();
        ensure!(len > 0, "an empty sequence has no options");
        ensure!(
            len <= self.max_positions,
            "a sequence of {len} tokens exceeds the encoder's {} positions",
            self.max_positions
        );
        ensure!(
            !seq.markers.is_empty(),
            "a sequence without markers has no options"
        );
        if let Some(&bad) = seq.markers.iter().find(|&&m| m >= len) {
            bail!("marker {bad} is outside the {len}-token sequence");
        }
        let device = self.type_emb.device();
        let ids = Tensor::new(seq.ids.as_slice(), device)?.unsqueeze(0)?;
        let mask = Tensor::ones((1, len), DType::U8, device)?;
        let mut states = self
            .encoder
            .forward(&ids, &mask)?
            .broadcast_add(&self.type_emb.get(qtype(seq.kind))?)?;
        for layer in &self.head {
            states = layer.forward(&states)?;
        }
        let markers = seq
            .markers
            .iter()
            .map(|&m| u32::try_from(m).context("a marker does not fit a u32"))
            .collect::<Result<Vec<u32>>>()?;
        let at_markers = states
            .squeeze(0)?
            .index_select(&Tensor::new(markers.as_slice(), device)?, 0)?;
        Ok(self.scorer.forward(&at_markers)?.squeeze(1)?.to_vec1()?)
    }
}

impl DecisionModel for Thx01 {
    /// Every sequence alone: its logits, divided by its bucket temperature, through the engine's softmax.
    fn decide(&mut self, sequences: &[DecisionSequence]) -> Result<Vec<Vec<f64>>> {
        sequences
            .iter()
            .map(|seq| {
                let logits = self.logits(seq)?;
                let temperature = self.temperatures.of(seq.kind, seq.markers.len());
                Ok(softmax(&logits, temperature))
            })
            .collect()
    }
}
