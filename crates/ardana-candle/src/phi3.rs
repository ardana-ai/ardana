// From candle-transformers 0.11.0, `src/models/phi3.rs`
// (https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/phi3.rs), copyright its
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
// - `RotaryEmbedding` scales `longrope` as transformers does (`_compute_longrope_parameters`): a frequency is divided
//   by its factor where upstream multiplies it, a sequence longer than `original_max_position_embeddings` takes the
//   long factors at every position and any other the short ones where upstream switches factors by position, and
//   sin and cos carry the attention factor, which upstream leaves out. Those tables are computed in F32 and then
//   converted, and a factor list of another length than the rotary frequencies is an error.
// - `Config` and the rope scaling types are candle-transformers' own, imported: this crate derives no `Deserialize`.
// - Attention uses the shared query chunks, synchronizing Metal after each to release completed score buffers.
// - No key-value cache: each prompt is decoded whole at offset 0, so layers retain no keys or values.
// - Left out, as nothing here uses it: `clear_kv_cache`.

//! Microsoft Phi-3 model implementation
//!
//! See Phi model details at:
//! - [Phi-3 Model](https://huggingface.co/microsoft/phi-3)
//!
//! The Phi series are decoder-only transformers designed for code and language tasks.
//! Key characteristics:
//! - Decoder-only transformer architecture
//! - RoPE embeddings
//! - Layer normalization
//! - QK normalization
//! - Mixed activation functions
//! - Improved context window handling
//!
//! References:
//! - [Hugging Face Implementation](https://huggingface.co/microsoft/phi-3)
//! - [Alternative Implementation](https://huggingface.co/microsoft/phi-3/tree/main)
//!

// This implementation is based on:
// https://huggingface.co/microsoft/Phi-3-mini-4k-instruct/blob/main/modeling_phi3.py
use candle_core::{D, DType, Device, IndexOp, Module, Result, Tensor};
use candle_nn::VarBuilder;
pub use candle_transformers::models::phi3::Config;
use candle_transformers::models::with_tracing::{Linear, RmsNorm, linear_no_bias as linear};
use candle_transformers::utils;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RotaryEmbedding {
    partial_dim: Option<usize>,
    sin: Tensor,
    cos: Tensor,
    // longrope: the sin and cos of a sequence longer than this many positions.
    long: Option<(usize, Tensor, Tensor)>,
}

impl RotaryEmbedding {
    pub fn new(dtype: DType, cfg: &Config, dev: &Device) -> Result<Self> {
        let partial_dim = cfg
            .partial_rotary_factor
            .as_ref()
            .map(|v| (v * cfg.head_dim() as f64) as usize);
        let dim = partial_dim.unwrap_or(cfg.head_dim());
        let max_seq_len = cfg.max_position_embeddings;
        let (sin, cos, long) = match cfg.rope_scaling.as_ref() {
            None => {
                let inv_freq: Vec<_> = (0..dim)
                    .step_by(2)
                    .map(|i| 1f32 / cfg.rope_theta.powf(i as f64 / dim as f64) as f32)
                    .collect();
                let (sin, cos) = crate::ops::rotary(&inv_freq, max_seq_len, 1.0, dtype, dev)?;
                (sin, cos, None)
            }
            Some(rope_scaling) => {
                let original_max_seq_len =
                    cfg.original_max_position_embeddings.unwrap_or(max_seq_len);
                let scale = max_seq_len as f64 / original_max_seq_len as f64;
                let attention_factor = if scale <= 1. {
                    1.
                } else {
                    (1. + scale.ln() / (original_max_seq_len as f64).ln()).sqrt()
                };
                let sin_cos = |factors: &[f32], seq_len: usize| -> Result<(Tensor, Tensor)> {
                    if factors.len() != dim.div_ceil(2) {
                        candle_core::bail!(
                            "{} longrope factors for {} rotary frequencies",
                            factors.len(),
                            dim.div_ceil(2)
                        )
                    }
                    let inv_freq: Vec<_> = (0..dim)
                        .step_by(2)
                        .zip(factors.iter())
                        .map(|(i, &f)| {
                            1f32 / (f * cfg.rope_theta.powf(i as f64 / dim as f64) as f32)
                        })
                        .collect();
                    crate::ops::rotary(&inv_freq, seq_len, attention_factor, dtype, dev)
                };
                if max_seq_len > original_max_seq_len {
                    let (sin, cos) = sin_cos(&rope_scaling.short_factor, original_max_seq_len)?;
                    let (sin_l, cos_l) = sin_cos(&rope_scaling.long_factor, max_seq_len)?;
                    (sin, cos, Some((original_max_seq_len, sin_l, cos_l)))
                } else {
                    let (sin, cos) = sin_cos(&rope_scaling.short_factor, max_seq_len)?;
                    (sin, cos, None)
                }
            }
        };
        Ok(Self {
            partial_dim,
            sin,
            cos,
            long,
        })
    }

    fn rope(&self, xs: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor> {
        let x = match self.partial_dim {
            None => candle_nn::rotary_emb::rope(&xs.contiguous()?, cos, sin)?,
            Some(dim) => {
                let xs_rot = xs.i((.., .., .., ..dim))?.contiguous()?;
                let xs_pass = xs.i((.., .., .., dim..))?;
                let xs_rot = candle_nn::rotary_emb::rope(&xs_rot, cos, sin)?;
                Tensor::cat(&[&xs_rot, &xs_pass], D::Minus1)?.contiguous()?
            }
        };
        Ok(x)
    }

    pub fn apply_rotary_emb_qkv(
        &self,
        q: &Tensor,
        k: &Tensor,
        seqlen_offset: usize,
    ) -> Result<(Tensor, Tensor)> {
        let (_b_sz, _h, seq_len, _n_embd) = q.dims4()?;
        let (sin, cos) = match &self.long {
            Some((original_max_seq_len, sin, cos))
                if seqlen_offset + seq_len > *original_max_seq_len =>
            {
                (sin, cos)
            }
            _ => (&self.sin, &self.cos),
        };
        let cos = cos.narrow(0, seqlen_offset, seq_len)?;
        let sin = sin.narrow(0, seqlen_offset, seq_len)?;
        let q_embed = self.rope(&q.contiguous()?, &cos, &sin)?;
        let k_embed = self.rope(&k.contiguous()?, &cos, &sin)?;
        Ok((q_embed, k_embed))
    }
}

#[derive(Debug, Clone)]
struct Attention {
    qkv_proj: Linear,
    o_proj: Linear,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    rotary_emb: Arc<RotaryEmbedding>,
}

impl Attention {
    fn new(rotary_emb: Arc<RotaryEmbedding>, cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let num_heads = cfg.num_attention_heads;
        let num_kv_heads = cfg.num_key_value_heads;
        let head_dim = cfg.head_dim();
        let op_size = num_heads * head_dim + 2 * num_kv_heads * head_dim;
        let qkv_proj = linear(cfg.hidden_size, op_size, vb.pp("qkv_proj"))?;
        let o_proj = linear(num_heads * head_dim, cfg.hidden_size, vb.pp("o_proj"))?;
        Ok(Self {
            qkv_proj,
            o_proj,
            rotary_emb,
            num_heads,
            num_kv_heads,
            num_kv_groups: num_heads / num_kv_heads,
            head_dim,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        attention_mask: Option<&Tensor>,
        seqlen_offset: usize,
    ) -> Result<Tensor> {
        let (b_sz, q_len, _) = xs.dims3()?;

        let qkv = self.qkv_proj.forward(xs)?;
        let query_pos = self.num_heads * self.head_dim;
        let query_states = qkv.narrow(D::Minus1, 0, query_pos)?;
        let key_states = qkv.narrow(D::Minus1, query_pos, self.num_kv_heads * self.head_dim)?;
        let value_states = qkv.narrow(
            D::Minus1,
            query_pos + self.num_kv_heads * self.head_dim,
            self.num_kv_heads * self.head_dim,
        )?;

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
            .reshape((b_sz, q_len, ()))?
            .apply(&self.o_proj)
    }
}

#[derive(Debug, Clone)]
struct Mlp {
    gate_up_proj: Linear,
    down_proj: Linear,
    act_fn: candle_nn::Activation,
    i_size: usize,
}

impl Mlp {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let hidden_size = cfg.hidden_size;
        let i_size = cfg.intermediate_size;
        let gate_up_proj = linear(hidden_size, 2 * i_size, vb.pp("gate_up_proj"))?;
        let down_proj = linear(i_size, hidden_size, vb.pp("down_proj"))?;
        Ok(Self {
            gate_up_proj,
            down_proj,
            act_fn: cfg.hidden_act,
            i_size,
        })
    }
}

impl Module for Mlp {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let up_states = xs.apply(&self.gate_up_proj)?;
        let gate = up_states.narrow(D::Minus1, 0, self.i_size)?;
        let up_states = up_states.narrow(D::Minus1, self.i_size, self.i_size)?;
        let up_states = (up_states * gate.apply(&self.act_fn))?;
        up_states.apply(&self.down_proj)
    }
}

#[derive(Debug, Clone)]
struct DecoderLayer {
    self_attn: Attention,
    mlp: Mlp,
    input_layernorm: RmsNorm,
    post_attention_layernorm: RmsNorm,
}

impl DecoderLayer {
    fn new(rotary_emb: Arc<RotaryEmbedding>, cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let self_attn = Attention::new(rotary_emb, cfg, vb.pp("self_attn"))?;
        let mlp = Mlp::new(cfg, vb.pp("mlp"))?;
        let input_layernorm =
            RmsNorm::new(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("input_layernorm"))?;
        let post_attention_layernorm = RmsNorm::new(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_attention_layernorm"),
        )?;
        Ok(Self {
            self_attn,
            mlp,
            input_layernorm,
            post_attention_layernorm,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        attention_mask: Option<&Tensor>,
        seqlen_offset: usize,
    ) -> Result<Tensor> {
        let residual = xs;
        let xs = self.input_layernorm.forward(xs)?;
        let xs = self.self_attn.forward(&xs, attention_mask, seqlen_offset)?;
        let xs = (xs + residual)?;
        let residual = &xs;
        let xs = xs.apply(&self.post_attention_layernorm)?.apply(&self.mlp)?;
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
}

impl Model {
    pub fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let vb_m = vb.pp("model");
        let embed_tokens =
            candle_nn::embedding(cfg.vocab_size, cfg.hidden_size, vb_m.pp("embed_tokens"))?;
        let rotary_emb = Arc::new(RotaryEmbedding::new(vb.dtype(), cfg, vb_m.device())?);
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        let vb_l = vb_m.pp("layers");
        for layer_idx in 0..cfg.num_hidden_layers {
            let layer = DecoderLayer::new(rotary_emb.clone(), cfg, vb_l.pp(layer_idx))?;
            layers.push(layer)
        }
        let norm = RmsNorm::new(cfg.hidden_size, cfg.rms_norm_eps, vb_m.pp("norm"))?;
        Ok(Self {
            embed_tokens,
            layers,
            norm,
            device: vb.device().clone(),
            dtype: vb.dtype(),
        })
    }

    fn prepare_decoder_attention_mask(
        &self,
        b_size: usize,
        tgt_len: usize,
        seqlen_offset: usize,
    ) -> Result<Tensor> {
        let mask: Vec<_> = (0..tgt_len)
            .flat_map(|i| (0..tgt_len).map(move |j| if i < j { f32::NEG_INFINITY } else { 0. }))
            .collect();
        let mask = Tensor::from_slice(&mask, (tgt_len, tgt_len), &self.device)?;
        let mask = if seqlen_offset > 0 {
            let mask0 = Tensor::zeros((tgt_len, seqlen_offset), DType::F32, &self.device)?;
            Tensor::cat(&[&mask0, &mask], D::Minus1)?
        } else {
            mask
        };
        mask.expand((b_size, 1, tgt_len, tgt_len + seqlen_offset))?
            .to_dtype(self.dtype)
    }

    pub fn forward(&self, input_ids: &Tensor, seqlen_offset: usize) -> Result<Tensor> {
        let (b_size, seq_len) = input_ids.dims2()?;
        let attention_mask = if seq_len <= 1 {
            None
        } else {
            let mask = self.prepare_decoder_attention_mask(b_size, seq_len, seqlen_offset)?;
            Some(mask)
        };
        let mut xs = self.embed_tokens.forward(input_ids)?;
        for layer in self.layers.iter() {
            xs = layer.forward(&xs, attention_mask.as_ref(), seqlen_offset)?
        }
        xs.apply(&self.norm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn longrope_precision(device: &Device) -> Result<()> {
        let cfg: Config = serde_json::from_value(serde_json::json!({
            "vocab_size": 20, "hidden_act": "silu", "hidden_size": 8, "intermediate_size": 16,
            "num_hidden_layers": 1, "num_attention_heads": 1, "num_key_value_heads": 1,
            "rms_norm_eps": 1e-5, "rope_theta": 10000.0, "bos_token_id": 1, "eos_token_id": 2,
            "rope_scaling": {"type": "longrope", "short_factor": [1.0, 1.0], "long_factor": [1.0, 4.0]},
            "max_position_embeddings": 6000, "original_max_position_embeddings": 4096,
            "partial_rotary_factor": 0.5, "tie_word_embeddings": true
        })).unwrap();
        let rotary = RotaryEmbedding::new(DType::BF16, &cfg, device)?;
        let scale = (1.0 + (6000f32 / 4096.0).ln() / 4096f32.ln()).sqrt();
        for (len, factor) in [(4096, 1.0), (4097, 4.0)] {
            let head = Tensor::new(&[[1f32, 1.0, 0.0, 0.0, 7.0, 7.0, 7.0, 7.0]], device)?;
            let heads = head
                .repeat((len, 1))?
                .reshape((1, 1, len, 8))?
                .to_dtype(DType::BF16)?;
            let (q, k) = rotary.apply_rotary_emb_qkv(&heads, &heads, 0)?;
            let q = q
                .reshape((len, 8))?
                .to_dtype(DType::F32)?
                .to_vec2::<f32>()?;
            assert_eq!(
                q,
                k.reshape((len, 8))?
                    .to_dtype(DType::F32)?
                    .to_vec2::<f32>()?
            );
            for position in [256, 257, len - 1] {
                let first = position as f32;
                let second = first * 0.01 / factor;
                let expected = [first.cos(), second.cos(), first.sin(), second.sin()];
                for (got, want) in q[position].iter().zip(expected.map(|x| x * scale)) {
                    assert!(
                        (got - want).abs() < 0.005,
                        "{len} tokens, position {position}: {got}, not {want}"
                    );
                }
                assert_eq!(&q[position][4..], &[7.0; 4]);
            }
            assert_ne!(q[256], q[257]);
        }
        Ok(())
    }

    #[test]
    fn bf16_longrope_applies_prompt_length_and_preserves_positions() -> Result<()> {
        longrope_precision(&Device::Cpu)
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn metal_bf16_longrope_applies_prompt_length_and_preserves_positions() -> Result<()> {
        longrope_precision(&Device::new_metal(0)?)
    }
}
