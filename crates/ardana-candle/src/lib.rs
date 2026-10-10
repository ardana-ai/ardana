//! The Candle runtime behind `ardana-core`'s runtime traits: safetensors causal LMs read from a Hugging Face snapshot
//! directory ([`ardana_core::snapshot`]), each architecture `config.json#architectures` names mapped to one
//! implementation ([`ARCHITECTURES`]), never chosen by a model name.
//!
//! A model computes on the CPU in F32; an Apple silicon build puts it on Metal in BF16 unless `gpu_layers` is 0. The
//! label logits are the head's rows for the label ids applied to the hidden states at the slots, in F32, so a prompt
//! never computes a whole vocabulary row.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use ardana_core::runtime::{LoadOptions, LoadedModel, Runtime};
use ardana_core::snapshot;
use candle_core::safetensors::MmapedSafetensors;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen3;
use serde_json::Value;

/// The `config.json#architectures` names this runtime implements.
pub const ARCHITECTURES: [&str; 1] = ["Qwen3ForCausalLM"];

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
        match architecture {
            "Qwen3ForCausalLM" => Ok(Box::new(Qwen3::load(
                weights, &config, opts, &device, dtype,
            )?)),
            other => bail!("no implementation of {other}"),
        }
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

/// A loaded `Qwen3ForCausalLM`.
pub struct Qwen3 {
    /// The decoder, never run itself: each prompt runs a clone, whose key-value cache starts empty (0.11.0 keeps
    /// `Model::clear_kv_cache` private).
    model: qwen3::Model,
    /// The output projection, `[vocab, hidden]`: `lm_head.weight`, or the embedding it is tied to.
    head: Tensor,
    vocab: usize,
    n_ctx: usize,
    dir: PathBuf,
}

impl std::fmt::Debug for Qwen3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Qwen3")
            .field("dir", &self.dir)
            .field("n_ctx", &self.n_ctx)
            .finish()
    }
}

impl Qwen3 {
    fn load(
        dir: &Path,
        config: &Value,
        opts: &LoadOptions,
        device: &Device,
        dtype: DType,
    ) -> Result<Qwen3> {
        let cfg: qwen3::Config = serde_json::from_value(config.clone()).with_context(|| {
            format!(
                "{} is no Qwen3 configuration",
                dir.join(snapshot::CONFIG).display()
            )
        })?;
        let files = snapshot::weight_files(dir).map_err(anyhow::Error::msg)?;
        let vb = read_tensors(&files, device, dtype)?;
        let model = qwen3::Model::new(&cfg, vb.clone())
            .with_context(|| format!("building Qwen3 from {}", dir.display()))?;
        let head = if cfg.tie_word_embeddings {
            "model.embed_tokens.weight"
        } else {
            "lm_head.weight"
        };
        let head = vb
            .get((cfg.vocab_size, cfg.hidden_size), head)
            .with_context(|| format!("reading the output projection {head}"))?;
        let n_ctx = (opts.n_ctx as usize).min(cfg.max_position_embeddings);
        ensure!(n_ctx > 0, "n_ctx must be greater than 0");
        Ok(Qwen3 {
            model,
            head,
            vocab: cfg.vocab_size,
            n_ctx,
            dir: dir.to_path_buf(),
        })
    }
}

impl LoadedModel for Qwen3 {
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
        let hidden = self.model.clone().forward(&input, 0)?.squeeze(0)?;
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
            err.to_string()
                .contains("[\"MambaForCausalLM\"]; candle implements Qwen3ForCausalLM"),
            "{err:#}"
        );
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
