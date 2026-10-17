// From candle-transformers 0.11.0, `src/models/olmo2.rs`
// (https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/olmo2.rs), copyright its
// authors, who publish candle under MIT OR Apache-2.0. Taken under Apache-2.0:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// Changed from upstream:
// - `Model::forward` returns the final-normed hidden states at every position. Upstream narrows to the last one
//   before `norm` and applies `lm_head`, which `Model` no longer holds: the readout reads the head, tied or not.
// - Olmo 3's layers: a layer is `Config::layer_types[layer_idx]`, `sliding_attention` or `full_attention`
//   (`config.json#layer_types`). A sliding layer attends under a mask that hides every key `Config::sliding_window`
//   or more positions before its query, as transformers (`sliding_window_overlay`) and llama.cpp (`is_masked_swa`)
//   do; upstream builds the causal mask alone and every layer attends over the whole prompt.
// - Olmo 3's YaRN rope scaling: the full-attention layers' rotary table takes `Config::rope_scaling`, a
//   `config.json#rope_scaling` of `rope_type` `yarn`, as transformers' `_compute_yarn_parameters` reads it (a
//   frequency inside the correction range of `beta_fast` and `beta_slow` is divided by `factor`, one outside it is
//   left as it is, between them a linear ramp; sin and cos are multiplied by `attention_factor`); a sliding layer
//   rotates with `rope_theta` unscaled, as transformers and llama.cpp do. Upstream has one unscaled table.
// - Rotary positions, frequencies, trig and the YaRN factor are computed in F32 before conversion to model dtype.
// - Attention uses the shared query chunks, synchronizing Metal after each to release completed score buffers.
// - No key-value cache: upstream keeps every layer's keys and values for a next call with a `seqlen_offset`, which
//   nothing here makes (a prompt is decoded alone, whole), and for a 5k-token prompt they hold 5.5 GB of its 32
//   MHA layers. `Attention::forward`, `DecoderLayer::forward` and `Model::forward` therefore take `&self`.
// - `Config` is built by `Config::new` from candle-transformers' own `olmo2::Config`, imported, and the three fields
//   above: this crate derives no `Deserialize`.
// - Left out, as nothing here uses them: `clear_kv_cache`, `Model`'s `lm_head`, and the `clip_qkv` upstream parses
//   and never applies. `MLP` is `Mlp`, which needs no lint allowed.

//! OLMo 2 (Open Language Model) implementation
//!
//! See OLMo 2 model details at:
//! - [Hugging Face Collection](https://huggingface.co/collections/allenai/olmo-2-674117b93ab84e98afc72edc)
//! - [OLMo 2 Paper](https://arxiv.org/abs/2501.00656)
//!
//!

use std::f64::consts::PI;
use std::sync::Arc;

use candle_core::{D, DType, Device, Module, Result, Tensor};
use candle_nn::{Activation, Linear, RmsNorm, VarBuilder, linear_b, linear_no_bias, rms_norm};
use candle_transformers::models::olmo2::Config as Olmo2Config;
use candle_transformers::utils;

/// One layer's attention, `config.json#layer_types`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType {
    SlidingAttention,
    FullAttention,
}

/// YaRN rope scaling of the full-attention layers, `config.json#rope_scaling` of `rope_type` `yarn`.
#[derive(Debug, Clone, PartialEq)]
pub struct Yarn {
    /// What a frequency inside the correction range is divided by.
    pub factor: f64,
    /// The positions the model was trained on, which bound the correction range.
    pub original_max_position_embeddings: usize,
    /// What sin and cos are multiplied by.
    pub attention_factor: f64,
    /// The rotations a frequency makes over the original positions above which it is left as it is.
    pub beta_fast: f64,
    /// The rotations below which it is divided by `factor`.
    pub beta_slow: f64,
}

impl Yarn {
    /// The rotary frequencies of a head `dim` wide over the base `rope_theta`, scaled as transformers'
    /// `_compute_yarn_parameters` scales them: pair `i` (of `dim / 2`) rotates `original_max_position_embeddings /
    /// (2 pi theta^(2 i / dim))` times over the original positions; a pair rotating `beta_fast` times or more keeps
    /// its frequency, one rotating `beta_slow` times or fewer takes it divided by `factor`, and a pair between the two
    /// (the range's ends floored and ceiled) mixes the two linearly.
    pub fn inv_freq(&self, dim: usize, rope_theta: f64) -> Vec<f32> {
        let correction_dim = |rotations: f64| {
            let positions = self.original_max_position_embeddings as f64;
            dim as f64 * (positions / (rotations * 2.0 * PI)).ln() / (2.0 * rope_theta.ln())
        };
        let low = correction_dim(self.beta_fast).floor().max(0.0);
        let high = correction_dim(self.beta_slow).ceil().min((dim - 1) as f64);
        let high = if high == low { high + 0.001 } else { high };
        (0..dim)
            .step_by(2)
            .map(|i| {
                let pos_freq = rope_theta.powf(i as f64 / dim as f64);
                let extrapolation = 1.0 / pos_freq;
                let interpolation = 1.0 / (self.factor * pos_freq);
                let ramp = (((i / 2) as f64 - low) / (high - low)).clamp(0.0, 1.0);
                (interpolation * ramp + extrapolation * (1.0 - ramp)) as f32
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub attention_bias: bool,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub rms_norm_eps: f64,
    pub hidden_act: Activation,
    pub max_position_embeddings: usize,
    pub rope_theta: f64,
    pub tie_word_embeddings: bool,
    /// Each layer's attention, in layer order.
    pub layer_types: Vec<LayerType>,
    /// The tokens a sliding layer attends over; needed when a layer slides.
    pub sliding_window: Option<usize>,
    /// The full-attention layers' rope scaling; none leaves every layer's table unscaled.
    pub rope_scaling: Option<Yarn>,
}

impl Config {
    /// `layer_types`, `sliding_window` and `rope_scaling` are `config.json`'s, which `Olmo2Config` does not read.
    pub fn new(
        cfg: Olmo2Config,
        layer_types: Vec<LayerType>,
        sliding_window: Option<usize>,
        rope_scaling: Option<Yarn>,
    ) -> Self {
        Self {
            vocab_size: cfg.vocab_size,
            hidden_size: cfg.hidden_size,
            intermediate_size: cfg.intermediate_size,
            attention_bias: cfg.attention_bias,
            num_hidden_layers: cfg.num_hidden_layers,
            num_attention_heads: cfg.num_attention_heads,
            num_key_value_heads: cfg.num_key_value_heads,
            rms_norm_eps: cfg.rms_norm_eps,
            hidden_act: cfg.hidden_act,
            max_position_embeddings: cfg.max_position_embeddings,
            rope_theta: cfg.rope_theta,
            tie_word_embeddings: cfg.tie_word_embeddings,
            layer_types,
            sliding_window,
            rope_scaling,
        }
    }
}

#[derive(Debug, Clone)]
struct RotaryEmbedding {
    sin: Tensor,
    cos: Tensor,
}

impl RotaryEmbedding {
    fn new(dtype: DType, cfg: &Config, dev: &Device, scaling: Option<&Yarn>) -> Result<Self> {
        let dim = cfg.hidden_size / cfg.num_attention_heads;
        let max_seq_len = cfg.max_position_embeddings;
        let (inv_freq, attention_factor) = match scaling {
            None => {
                let inv_freq: Vec<_> = (0..dim)
                    .step_by(2)
                    .map(|i| 1f32 / cfg.rope_theta.powf(i as f64 / dim as f64) as f32)
                    .collect();
                (inv_freq, 1.0)
            }
            Some(yarn) => (yarn.inv_freq(dim, cfg.rope_theta), yarn.attention_factor),
        };
        let (sin, cos) = crate::ops::rotary(&inv_freq, max_seq_len, attention_factor, dtype, dev)?;
        Ok(Self { sin, cos })
    }

    fn apply_rotary_emb_qkv(
        &self,
        q: &Tensor,
        k: &Tensor,
        seqlen_offset: usize,
    ) -> Result<(Tensor, Tensor)> {
        let (_b_sz, _h, seq_len, _n_embd) = q.dims4()?;
        let cos = self.cos.narrow(0, seqlen_offset, seq_len)?;
        let sin = self.sin.narrow(0, seqlen_offset, seq_len)?;
        let q_embed = candle_nn::rotary_emb::rope(&q.contiguous()?, &cos, &sin)?;
        let k_embed = candle_nn::rotary_emb::rope(&k.contiguous()?, &cos, &sin)?;
        Ok((q_embed, k_embed))
    }
}

#[derive(Debug, Clone)]
struct Mlp {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
    act_fn: Activation,
}

impl Mlp {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let hidden_sz = cfg.hidden_size;
        let intermediate_sz = cfg.intermediate_size;
        let gate_proj = linear_no_bias(hidden_sz, intermediate_sz, vb.pp("gate_proj"))?;
        let up_proj = linear_no_bias(hidden_sz, intermediate_sz, vb.pp("up_proj"))?;
        let down_proj = linear_no_bias(intermediate_sz, hidden_sz, vb.pp("down_proj"))?;
        Ok(Self {
            gate_proj,
            up_proj,
            down_proj,
            act_fn: cfg.hidden_act,
        })
    }
}

impl Module for Mlp {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let lhs = xs.apply(&self.gate_proj)?.apply(&self.act_fn)?;
        let rhs = xs.apply(&self.up_proj)?;
        (lhs * rhs)?.apply(&self.down_proj)
    }
}

#[derive(Debug, Clone)]
struct Attention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    hidden_size: usize,
    rotary_emb: Arc<RotaryEmbedding>,
}

impl Attention {
    fn new(rotary_emb: Arc<RotaryEmbedding>, cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let hidden_sz = cfg.hidden_size;
        let num_heads = cfg.num_attention_heads;
        let num_kv_heads = cfg.num_key_value_heads;
        let num_kv_groups = num_heads / num_kv_heads;
        let head_dim = hidden_sz / num_heads;
        let b = cfg.attention_bias;
        let q_proj = linear_b(hidden_sz, num_heads * head_dim, b, vb.pp("q_proj"))?;
        let k_proj = linear_b(hidden_sz, num_kv_heads * head_dim, b, vb.pp("k_proj"))?;
        let v_proj = linear_b(hidden_sz, num_kv_heads * head_dim, b, vb.pp("v_proj"))?;
        let o_proj = linear_b(num_heads * head_dim, hidden_sz, b, vb.pp("o_proj"))?;
        let q_norm = rms_norm(hidden_sz, cfg.rms_norm_eps, vb.pp("q_norm"))?;
        let k_norm = rms_norm(num_kv_heads * head_dim, cfg.rms_norm_eps, vb.pp("k_norm"))?;
        Ok(Self {
            q_proj,
            k_proj,
            v_proj,
            o_proj,
            q_norm,
            k_norm,
            num_heads,
            num_kv_heads,
            num_kv_groups,
            head_dim,
            hidden_size: hidden_sz,
            rotary_emb,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        attention_mask: Option<&Tensor>,
        seqlen_offset: usize,
    ) -> Result<Tensor> {
        let (b_sz, q_len, _) = xs.dims3()?;

        let query_states = self.q_proj.forward(xs)?;
        let key_states = self.k_proj.forward(xs)?;
        let value_states = self.v_proj.forward(xs)?;

        let query_states = self.q_norm.forward(&query_states)?;
        let key_states = self.k_norm.forward(&key_states)?;

        let query_states = query_states
            .reshape((b_sz, q_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?;
        let key_states = key_states
            .reshape((b_sz, q_len, self.num_kv_heads, self.head_dim))?
            .transpose(1, 2)?;
        let value_states = value_states
            .reshape((b_sz, q_len, self.num_kv_heads, self.head_dim))?
            .transpose(1, 2)?;

        let (query_states, key_states) =
            self.rotary_emb
                .apply_rotary_emb_qkv(&query_states, &key_states, seqlen_offset)?;

        let key_states = utils::repeat_kv(key_states, self.num_kv_groups)?.contiguous()?;
        let value_states = utils::repeat_kv(value_states, self.num_kv_groups)?.contiguous()?;

        let attn_output = crate::ops::attention(
            &query_states,
            &key_states,
            &value_states,
            attention_mask,
            1f64 / f64::sqrt(self.head_dim as f64),
            None,
        )?;
        attn_output
            .transpose(1, 2)?
            .reshape((b_sz, q_len, self.hidden_size))?
            .apply(&self.o_proj)
    }
}

#[derive(Debug, Clone)]
struct DecoderLayer {
    self_attn: Attention,
    mlp: Mlp,
    post_attention_layernorm: RmsNorm,
    post_feedforward_layernorm: RmsNorm,
    layer_type: LayerType,
}

impl DecoderLayer {
    fn new(
        rotary_emb: Arc<RotaryEmbedding>,
        cfg: &Config,
        layer_type: LayerType,
        vb: VarBuilder,
    ) -> Result<Self> {
        let self_attn = Attention::new(rotary_emb, cfg, vb.pp("self_attn"))?;
        let mlp = Mlp::new(cfg, vb.pp("mlp"))?;
        let post_feedforward_layernorm = rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_feedforward_layernorm"),
        )?;
        let post_attention_layernorm = rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_attention_layernorm"),
        )?;
        Ok(Self {
            self_attn,
            mlp,
            post_attention_layernorm,
            post_feedforward_layernorm,
            layer_type,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        attention_mask: Option<&Tensor>,
        seqlen_offset: usize,
    ) -> Result<Tensor> {
        let residual = xs;
        let xs = self.self_attn.forward(xs, attention_mask, seqlen_offset)?;
        let xs = self.post_attention_layernorm.forward(&xs)?;
        let xs = (xs + residual)?;
        let residual = &xs;
        let xs = self.mlp.forward(&xs)?;
        let xs = self.post_feedforward_layernorm.forward(&xs)?;
        residual + xs
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    embed_tokens: candle_nn::Embedding,
    layers: Vec<DecoderLayer>,
    norm: RmsNorm,
    device: Device,
    dtype: DType,
    sliding_window: Option<usize>,
}

impl Model {
    pub fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let vb_m = vb.pp("model");
        let embed_tokens =
            candle_nn::embedding(cfg.vocab_size, cfg.hidden_size, vb_m.pp("embed_tokens"))?;
        let rotary_emb = Arc::new(RotaryEmbedding::new(vb.dtype(), cfg, vb_m.device(), None)?);
        let scaled_rotary_emb = match &cfg.rope_scaling {
            None => rotary_emb.clone(),
            Some(yarn) => Arc::new(RotaryEmbedding::new(
                vb.dtype(),
                cfg,
                vb_m.device(),
                Some(yarn),
            )?),
        };
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        let vb_l = vb_m.pp("layers");
        for layer_idx in 0..cfg.num_hidden_layers {
            let Some(&layer_type) = cfg.layer_types.get(layer_idx) else {
                candle_core::bail!(
                    "layer_types names {} layers, and the model has {}",
                    cfg.layer_types.len(),
                    cfg.num_hidden_layers
                )
            };
            let rotary_emb = match layer_type {
                LayerType::SlidingAttention => {
                    if cfg.sliding_window.is_none() {
                        candle_core::bail!(
                            "layer {layer_idx} is sliding_attention without a sliding_window"
                        )
                    }
                    rotary_emb.clone()
                }
                LayerType::FullAttention => scaled_rotary_emb.clone(),
            };
            let layer = DecoderLayer::new(rotary_emb, cfg, layer_type, vb_l.pp(layer_idx))?;
            layers.push(layer)
        }
        let norm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb_m.pp("norm"))?;
        Ok(Self {
            embed_tokens,
            layers,
            norm,
            device: vb.device().clone(),
            dtype: vb.dtype(),
            sliding_window: cfg.sliding_window,
        })
    }

    fn prepare_decoder_attention_mask(
        &self,
        b_size: usize,
        tgt_len: usize,
        seqlen_offset: usize,
        sliding_window: Option<usize>,
    ) -> Result<Tensor> {
        let mask: Vec<_> = (0..tgt_len)
            .flat_map(|i| {
                (0..tgt_len).map(move |j| {
                    if i < j || sliding_window.is_some_and(|window| j + window <= i) {
                        f32::NEG_INFINITY
                    } else {
                        0.
                    }
                })
            })
            .collect();
        let mask = Tensor::from_slice(&mask, (tgt_len, tgt_len), &self.device)?;
        let mask = if seqlen_offset > 0 {
            let mask0 = Tensor::zeros((tgt_len, seqlen_offset), self.dtype, &self.device)?;
            Tensor::cat(&[&mask0, &mask], D::Minus1)?
        } else {
            mask
        };
        mask.expand((b_size, 1, tgt_len, tgt_len + seqlen_offset))?
            .to_dtype(self.dtype)
    }

    pub fn forward(&self, input_ids: &Tensor, seqlen_offset: usize) -> Result<Tensor> {
        let (b_size, seq_len) = input_ids.dims2()?;
        let (attention_mask, sliding_attention_mask) = if seq_len <= 1 {
            (None, None)
        } else {
            let mask = self.prepare_decoder_attention_mask(b_size, seq_len, seqlen_offset, None)?;
            let sliding_mask = match self.sliding_window {
                None => None,
                Some(window) => Some(self.prepare_decoder_attention_mask(
                    b_size,
                    seq_len,
                    seqlen_offset,
                    Some(window),
                )?),
            };
            (Some(mask), sliding_mask)
        };
        let mut xs = self.embed_tokens.forward(input_ids)?;
        for layer in &self.layers {
            let mask = match layer.layer_type {
                LayerType::SlidingAttention => &sliding_attention_mask,
                LayerType::FullAttention => &attention_mask,
            };
            xs = layer.forward(&xs, mask.as_ref(), seqlen_offset)?
        }
        xs.apply(&self.norm)
    }
}
