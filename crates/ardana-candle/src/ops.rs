use candle_core::{DType, Device, Result, Tensor};

/// A score tensor covers at most this many queries; keys still cover the whole prompt.
const QUERY_CHUNK: usize = 512;

pub(crate) fn rotary(
    inv_freq: &[f32],
    positions: usize,
    attention_factor: f64,
    dtype: DType,
    device: &Device,
) -> Result<(Tensor, Tensor)> {
    let inv_freq = Tensor::from_slice(inv_freq, (1, inv_freq.len()), device)?;
    let positions = Tensor::arange(0u32, positions as u32, device)?
        .to_dtype(DType::F32)?
        .reshape((positions, 1))?;
    let freqs = positions.matmul(&inv_freq)?;
    Ok((
        (freqs.sin()? * attention_factor)?.to_dtype(dtype)?,
        (freqs.cos()? * attention_factor)?.to_dtype(dtype)?,
    ))
}

/// Whole-prompt attention, with each query chunk completed before Metal can retain another chunk's scores.
pub(crate) fn attention(
    queries: &Tensor,
    keys: &Tensor,
    values: &Tensor,
    mask: Option<&Tensor>,
    scale: f64,
    softcap: Option<f64>,
) -> Result<Tensor> {
    let q_len = queries.dim(2)?;
    let keys = keys.transpose(2, 3)?;
    let mut chunks = Vec::with_capacity(q_len.div_ceil(QUERY_CHUNK));
    for start in (0..q_len).step_by(QUERY_CHUNK) {
        let len = QUERY_CHUNK.min(q_len - start);
        let output = {
            let queries = queries.narrow(2, start, len)?;
            let scores = (queries.matmul(&keys)? * scale)?;
            let scores = match softcap {
                None => scores,
                Some(cap) => ((scores / cap)?.tanh()? * cap)?,
            };
            let scores = match mask {
                None => scores,
                Some(mask) => scores.broadcast_add(&mask.narrow(2, start, len)?)?,
            };
            candle_nn::ops::softmax_last_dim(&scores)?.matmul(values)?
        };
        chunks.push(output);
        // Score temporaries have dropped. Candle waits for their command buffers, then removes unused buffers
        // from its pools and Metal residency set; without this, asynchronous chunks/layers retain their scores.
        if queries.device().is_metal() {
            queries.device().synchronize()?;
        }
    }
    Tensor::cat(&chunks, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotary_precision(device: &Device) -> Result<()> {
        let frequencies = [1f32, 0.012345678, 0.00012345678];
        let (sin, cos) = rotary(&frequencies, 5500, 1.207944, DType::BF16, device)?;
        assert_eq!(sin.dtype(), DType::BF16);
        let sin = sin.to_dtype(DType::F32)?.to_vec2::<f32>()?;
        let cos = cos.to_dtype(DType::F32)?.to_vec2::<f32>()?;
        for position in [257, 4097, 5483, 255, 256, 4096] {
            for (pair, &frequency) in frequencies.iter().enumerate() {
                let angle = position as f32 * frequency;
                assert!(
                    (sin[position][pair] - angle.sin() * 1.207944).abs() < 0.005,
                    "sin position {position}, pair {pair}: {}",
                    sin[position][pair]
                );
                assert!(
                    (cos[position][pair] - angle.cos() * 1.207944).abs() < 0.005,
                    "cos position {position}, pair {pair}: {}",
                    cos[position][pair]
                );
            }
        }
        assert_ne!(sin[256], sin[257]);
        assert_ne!(sin[4096], sin[4097]);
        Ok(())
    }

    #[test]
    fn rotary_keeps_long_positions_in_f32() -> Result<()> {
        rotary_precision(&Device::Cpu)
    }

    fn inputs(len: usize, device: &Device, dtype: DType) -> Result<(Tensor, Tensor, Tensor)> {
        let values = Tensor::arange(0u32, (2 * 2 * len * 8) as u32, device)?
            .to_dtype(DType::F32)?
            .reshape((2, 2, len, 8))?;
        Ok((
            (&values * 0.137)?.sin()?.to_dtype(dtype)?,
            (&values * 0.173)?.cos()?.to_dtype(dtype)?,
            (&values * 0.197)?.sin()?.to_dtype(dtype)?,
        ))
    }

    fn mask(len: usize, window: usize, device: &Device, dtype: DType) -> Result<Tensor> {
        let values: Vec<_> = (0..len)
            .flat_map(|q| {
                (0..len).map(move |k| {
                    if k > q || q - k >= window {
                        f32::NEG_INFINITY
                    } else {
                        0.0
                    }
                })
            })
            .collect();
        Tensor::from_vec(values, (1, 1, len, len), device)?.to_dtype(dtype)
    }

    fn dense(
        q: &Tensor,
        k: &Tensor,
        v: &Tensor,
        mask: Option<&Tensor>,
        softcap: Option<f64>,
    ) -> Result<Tensor> {
        let scores = (q.matmul(&k.transpose(2, 3)?)? * 0.5)?;
        let scores = match softcap {
            None => scores,
            Some(cap) => ((scores / cap)?.tanh()? * cap)?,
        };
        let scores = match mask {
            None => scores,
            Some(mask) => scores.broadcast_add(mask)?,
        };
        candle_nn::ops::softmax_last_dim(&scores)?.matmul(v)
    }

    fn equivalent(device: &Device, dtype: DType, tolerance: f32) -> Result<()> {
        for len in [1, 511, 512, 513, 1025] {
            let (q, k, v) = inputs(len, device, dtype)?;
            for window in [None, Some(len), Some(17)] {
                let mask = window.map(|w| mask(len, w, device, dtype)).transpose()?;
                for softcap in [None, Some(0.7)] {
                    let expected = dense(&q, &k, &v, mask.as_ref(), softcap)?;
                    let actual = attention(&q, &k, &v, mask.as_ref(), 0.5, softcap)?;
                    let gap = (actual - expected)?
                        .to_dtype(DType::F32)?
                        .abs()?
                        .flatten_all()?
                        .max(0)?
                        .to_scalar::<f32>()?;
                    assert!(
                        gap <= tolerance,
                        "{len} queries, window {window:?}, cap {softcap:?}: {gap}"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn attention_chunks_match_dense_causal_sliding_and_softcapping() -> Result<()> {
        equivalent(&Device::Cpu, DType::F32, 1e-5)
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn metal_bf16_rotary_and_attention_release_score_buffers() -> Result<()> {
        let device = Device::new_metal(0)?;
        rotary_precision(&device)?;
        equivalent(&device, DType::BF16, 0.004)?;
        let (q, k, v) = inputs(2049, &device, DType::BF16)?;
        let mask = mask(2049, 2049, &device, DType::BF16)?;
        device.synchronize()?;
        let metal = device.as_metal_device()?.device();
        let before = metal.current_allocated_size();
        let mut outputs = Vec::new();
        for _ in 0..3 {
            outputs.push(attention(&q, &k, &v, Some(&mask), 0.5, None)?);
            // Inspect before any CPU readback (which would itself flush and release the queued scores).
            let retained = metal.current_allocated_size().saturating_sub(before);
            assert!(
                retained < 8 * 1024 * 1024,
                "attention retained {retained} Metal bytes"
            );
        }
        Ok(())
    }
}
