//! Where a fine-tune trains, and whether it fits there (Q4, Q16, Q23). torch in the training environment says which
//! devices it finds (`devices.py`), and `--device auto` takes CUDA, then MPS, then the CPU. A run trains in bfloat16 on
//! CUDA and MPS, recomputing its activations as TRL does by default, and in float32 on the CPU, keeping them. Before the
//! checkpoint downloads, the memory it needs is estimated from the checkpoint's bytes in its listing and its
//! `config.json` ([`Shape`]) and held against the device's: the GPU's on CUDA, the working set Metal recommends Apple's
//! GPU on MPS, the machine's on the CPU. On CUDA a run that does not fit in bfloat16 but fits with its weights in 4-bit
//! NF4 trains with QLoRA; any other run that does not fit is refused, naming both numbers.

use anyhow::{Context, Result, bail, ensure};
use ardana_api::human_size;
use clap::ValueEnum;
use serde_json::Value;

use super::{Interrupt, LORA_RANK, Work, env};

/// The rows of a training step: TRL's `per_device_train_batch_size`, 8 by default, which the run keeps.
const BATCH: u64 = 8;
/// What a token keeps alive in a decoder layer until the backward pass, in values of the training dtype per unit of
/// the layer's hidden and intermediate sizes. It bounds the trainer's peak resident memory measured on the CPU in
/// float32, beyond the weights, the adapter and the runtime, on steps of 8 rows: 13 for Qwen3-0.6B (rows of 128 tokens,
/// a 10.5 GB peak) and 23 for decider:0.8b, whose Qwen3.5 linear-attention layers run transformers' pure-torch path off
/// CUDA (rows of 261 tokens, a 25.0 GB peak).
const ACTIVATIONS: u64 = 23;
/// Python with torch, transformers and TRL, and on a GPU its context.
const RUNTIME: u64 = 1_000_000_000;
/// The same on MPS, with Metal's buffers, which torch's allocator keeps cached: the trainer's peak footprint
/// (`vmmap`'s) passed the estimate made with [`RUNTIME`] by 0.9 to 2.8 GB on five bases of 0.6B to 3B parameters
/// (qwen3:0.6b, llama-3.2:1b, lfm2.5:1.2b, minicpm5:1b, smollm3:3b on 8 rows of 136 tokens), the same at every size.
const MPS_RUNTIME: u64 = 4_000_000_000;

/// `--device`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Device {
    /// CUDA, else MPS, else the CPU, as torch finds them.
    Auto,
    /// The first NVIDIA GPU.
    Cuda,
    /// Apple's GPU.
    Mps,
    Cpu,
}

impl Device {
    /// The name `--device` and `finetune.json#device` give it.
    pub fn name(self) -> &'static str {
        match self {
            Device::Auto => "auto",
            Device::Cuda => "cuda",
            Device::Mps => "mps",
            Device::Cpu => "cpu",
        }
    }
}

/// The devices torch finds, as `devices.py` prints them.
#[derive(Debug, Clone)]
pub struct Found {
    /// The first CUDA device's name and memory.
    cuda: Option<(String, u64)>,
    /// The memory Metal recommends Apple's GPU use (`recommendedMaxWorkingSetSize`), when torch finds the device.
    mps: Option<u64>,
    /// The machine's physical memory.
    memory: u64,
}

impl Found {
    /// The devices found, for a message: `cuda (<GPU>, <memory>)`, `mps` and `cpu`, in `--device auto`'s order.
    fn names(&self) -> String {
        let mut names = Vec::new();
        if let Some((gpu, memory)) = &self.cuda {
            names.push(format!("cuda ({gpu}, {})", human_size(*memory)));
        }
        if self.mps.is_some() {
            names.push("mps".to_string());
        }
        names.push("cpu".to_string());
        names.join(", ")
    }
}

/// Asks torch in the environment which devices it finds.
pub async fn probe(env: &env::Env, work: &Work, interrupt: &mut Interrupt) -> Result<Found> {
    let script = work.dir.join("devices.py");
    std::fs::write(&script, include_str!("devices.py"))?;
    let mut cmd = super::python(env, work);
    cmd.arg("-I").arg(&script).kill_on_drop(true);
    let output = interrupt
        .or(async { cmd.output().await.context("starting devices.py") })
        .await?;
    ensure!(
        output.status.success(),
        "asking torch for its devices (devices.py) failed ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let found: Value =
        serde_json::from_slice(&output.stdout).context("reading devices.py's answer")?;
    let cuda = match &found["cuda"] {
        Value::Null => None,
        gpu => Some((
            gpu["name"]
                .as_str()
                .context("devices.py's CUDA device name")?
                .to_string(),
            gpu["memory"].as_u64().context("devices.py's CUDA memory")?,
        )),
    };
    let mps = match &found["mps"] {
        Value::Null => None,
        mps => Some(mps.as_u64().context("devices.py's MPS memory")?),
    };
    Ok(Found {
        cuda,
        mps,
        memory: found["memory"].as_u64().context("devices.py's memory")?,
    })
}

/// The device `asked` names, `--device auto` the first of CUDA, MPS and the CPU torch finds; a device torch does not
/// find is refused, naming those it does.
pub fn pick(asked: Device, found: &Found) -> Result<Device> {
    let device = match asked {
        Device::Auto if found.cuda.is_some() => Device::Cuda,
        Device::Auto if found.mps.is_some() => Device::Mps,
        Device::Auto => Device::Cpu,
        asked => asked,
    };
    let there = match device {
        Device::Cuda => found.cuda.is_some(),
        Device::Mps => found.mps.is_some(),
        Device::Auto | Device::Cpu => true,
    };
    ensure!(
        there,
        "--device {}: torch finds no {} device on this machine; it finds {}",
        device.name(),
        match device {
            Device::Cuda => "CUDA",
            _ => "MPS",
        },
        found.names()
    );
    Ok(device)
}

/// What a checkpoint's memory estimate reads: its parameters and the shape of its decoder.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    params: u64,
    /// The parameters of the token embeddings and, when not tied to them, the LM head.
    embeddings: u64,
    layers: u64,
    hidden: u64,
    intermediate: u64,
}

impl Shape {
    /// The shape of a checkpoint of `bytes` in safetensors and this `config.json` (its `text_config` for a multimodal
    /// one): its parameters are its bytes at the dtype the config names (2 bytes a parameter when it names none or
    /// another than float32), its embeddings `vocab_size` × `hidden_size`, twice unless `tie_word_embeddings`, and an
    /// `intermediate_size` it does not name four times `hidden_size`.
    pub fn of(bytes: u64, config: &Value) -> Result<Shape> {
        let text = if config["text_config"].is_object() {
            &config["text_config"]
        } else {
            config
        };
        let size = |key: &str| {
            text[key].as_u64().with_context(|| {
                format!(
                    "the checkpoint's config.json names no {key}, which the memory estimate reads"
                )
            })
        };
        let (hidden, layers, vocab) = (
            size("hidden_size")?,
            size("num_hidden_layers")?,
            size("vocab_size")?,
        );
        let intermediate = text["intermediate_size"].as_u64().unwrap_or(4 * hidden);
        let stored = [config, text]
            .iter()
            .flat_map(|config| ["dtype", "torch_dtype"].map(|key| config[key].as_str()))
            .flatten()
            .next();
        let width = if stored == Some("float32") { 4 } else { 2 };
        let tied = [config, text]
            .iter()
            .find_map(|config| config["tie_word_embeddings"].as_bool())
            .unwrap_or(false);
        let params = bytes / width;
        Ok(Shape {
            params,
            embeddings: (vocab * hidden * if tied { 1 } else { 2 }).min(params),
            layers,
            hidden,
            intermediate,
        })
    }

    /// The bytes a run needs to train it with rows of up to `longest` tokens: the weights at `width` bytes a parameter
    /// (under `four_bit` the linear layers' in 4-bit NF4 with a 4-byte scale per 64, the embeddings' in 16 bits); the
    /// LoRA adapter of every linear layer with its gradients and Adam's two moments, 16 bytes a parameter; the
    /// activations of a step of [`BATCH`] such rows at `width`, every layer's kept, or under `checkpointing` each
    /// layer's input and one layer's whole; and the `runtime`.
    fn needs(
        self,
        width: u64,
        four_bit: bool,
        checkpointing: bool,
        longest: u64,
        runtime: u64,
    ) -> u64 {
        let linear = self.params - self.embeddings;
        let weights = if four_bit {
            linear * 9 / 16 + self.embeddings * 2
        } else {
            self.params * width
        };
        let adapter = linear * 2 * u64::from(LORA_RANK) / self.hidden * 16;
        let layer = ACTIVATIONS * (self.hidden + self.intermediate) * width;
        let token = if checkpointing {
            self.layers * self.hidden * width + layer
        } else {
            self.layers * layer
        };
        weights + adapter + BATCH * longest * token + runtime
    }
}

/// How a run trains: on a device, in a dtype, with QLoRA or not.
#[derive(Debug, Clone, Copy)]
pub struct Training {
    pub device: Device,
    /// `bfloat16` or `float32`, as `--dtype` and `finetune.json#dtype` name it.
    pub dtype: &'static str,
    pub qlora: bool,
}

/// How `base`, of `shape`, trains on `device` with rows of up to `longest` tokens, or why it cannot: bfloat16 on CUDA
/// when it fits the GPU, else QLoRA when that fits; bfloat16 on MPS when it fits the working set Metal recommends the
/// GPU, past which the machine swaps; float32 on the CPU when it fits the machine's memory. Says what it picked.
pub fn fit(
    device: Device,
    found: &Found,
    shape: Shape,
    longest: u64,
    base: &str,
) -> Result<Training> {
    let (dtype, width, checkpointing, runtime) = match device {
        Device::Cpu => ("float32", 4, false, RUNTIME),
        Device::Mps => ("bfloat16", 2, true, MPS_RUNTIME),
        _ => ("bfloat16", 2, true, RUNTIME),
    };
    let needs = shape.needs(width, false, checkpointing, longest, runtime);
    let training = Training {
        device,
        dtype,
        qlora: false,
    };
    if let (Device::Cuda, Some((gpu, has))) = (device, &found.cuda) {
        let has = *has;
        if needs <= has {
            eprintln!(
                "{base} trains on cuda in {dtype}, needing about {} of the {} of {gpu}",
                human_size(needs),
                human_size(has)
            );
            return Ok(training);
        }
        let quantized = shape.needs(width, true, checkpointing, longest, runtime);
        if quantized <= has {
            eprintln!(
                "{base} needs about {} to train in {dtype}, more than the {} of {gpu}: training with QLoRA, its weights \
                 in 4-bit NF4, needing about {}",
                human_size(needs),
                human_size(has),
                human_size(quantized)
            );
            return Ok(Training {
                qlora: true,
                ..training
            });
        }
        bail!(
            "{base} needs about {} of GPU memory to train on cuda in {dtype}, and about {} with QLoRA, more than the {} \
             of {gpu}; pick a smaller base",
            human_size(needs),
            human_size(quantized),
            human_size(has)
        );
    }
    if let (Device::Mps, Some(has)) = (device, found.mps) {
        ensure!(
            needs <= has,
            "{base} needs about {} of memory to train on mps in {dtype}, more than the {} Metal recommends Apple's \
             GPU use on this machine (of its {}); pick a smaller base",
            human_size(needs),
            human_size(has),
            human_size(found.memory)
        );
        eprintln!(
            "{base} trains on mps in {dtype}, needing about {} of the {} Metal recommends Apple's GPU use on this \
             machine",
            human_size(needs),
            human_size(has)
        );
        return Ok(training);
    }
    let has = found.memory;
    if needs > has {
        bail!(
            "{base} needs about {} of memory to train on {} in {dtype}, more than the {} this machine has; pick a \
             smaller base",
            human_size(needs),
            device.name(),
            human_size(has)
        );
    }
    eprintln!(
        "{base} trains on {} in {dtype}, needing about {} of the {} of memory this machine has",
        device.name(),
        human_size(needs),
        human_size(has)
    );
    Ok(training)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R5.2: `--device auto` takes CUDA when torch finds it, else MPS, else the CPU (Q4); a device named is taken when
    /// torch finds it, and refused when it does not, naming the devices it finds in auto's order.
    #[test]
    fn pick_takes_cuda_then_mps_then_cpu() {
        const GPU_MEMORY: u64 = 24_000_000_000;
        let machine = |cuda: bool, mps: bool| Found {
            cuda: cuda.then(|| ("NVIDIA L4".to_string(), GPU_MEMORY)),
            mps: mps.then_some(48_000_000_000),
            memory: 64_000_000_000,
        };
        for (cuda, mps, auto) in [
            (true, true, Device::Cuda),
            (true, false, Device::Cuda),
            (false, true, Device::Mps),
            (false, false, Device::Cpu),
        ] {
            let found = machine(cuda, mps);
            assert_eq!(pick(Device::Auto, &found).unwrap(), auto, "{found:?}");
            assert_eq!(pick(Device::Cpu, &found).unwrap(), Device::Cpu, "{found:?}");
        }
        assert_eq!(
            pick(Device::Cuda, &machine(true, true)).unwrap(),
            Device::Cuda
        );
        assert_eq!(
            pick(Device::Mps, &machine(true, true)).unwrap(),
            Device::Mps
        );

        let refused = |asked, found: Found| pick(asked, &found).unwrap_err().to_string();
        assert_eq!(
            refused(Device::Cuda, machine(false, false)),
            "--device cuda: torch finds no CUDA device on this machine; it finds cpu"
        );
        assert_eq!(
            refused(Device::Cuda, machine(false, true)),
            "--device cuda: torch finds no CUDA device on this machine; it finds mps, cpu"
        );
        assert_eq!(
            refused(Device::Mps, machine(false, false)),
            "--device mps: torch finds no MPS device on this machine; it finds cpu"
        );
        assert_eq!(
            refused(Device::Mps, machine(true, false)),
            format!(
                "--device mps: torch finds no MPS device on this machine; it finds cuda (NVIDIA L4, {}), cpu",
                human_size(GPU_MEMORY)
            )
        );
    }

    /// Q16 on Apple Silicon: a run on MPS is held against the working set Metal recommends the GPU (55.7 GB of the M1
    /// Max's 68.7 GB), past which the machine swaps, with [`MPS_RUNTIME`]; the CPU against the machine's memory. A base
    /// of 54 GB in bfloat16 (qwen3.8:27b's text tower: 64 layers of 5,120 and 17,408) fits the machine's memory, not
    /// the working set, so it is refused on MPS, and on the CPU in float32.
    #[test]
    fn fit_holds_mps_to_metals_working_set() {
        let machine = Found {
            cuda: None,
            mps: Some(55_662_788_608),
            memory: 68_719_476_736,
        };
        let big = Shape::of(
            54_000_000_000,
            &serde_json::json!({"hidden_size": 5120, "intermediate_size": 17408, "num_hidden_layers": 64,
                "vocab_size": 248320, "tie_word_embeddings": false, "dtype": "bfloat16"}),
        )
        .unwrap();
        let needs = big.needs(2, false, true, 136, MPS_RUNTIME);
        assert!(needs > 55_662_788_608 && needs < 68_719_476_736, "{needs}");
        let err = fit(Device::Mps, &machine, big, 136, "big")
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            format!(
                "big needs about {} of memory to train on mps in bfloat16, more than the 55.7 GB Metal recommends \
                 Apple's GPU use on this machine (of its 68.7 GB); pick a smaller base",
                human_size(needs)
            )
        );
        let err = fit(Device::Cpu, &machine, big, 136, "big")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(
                "of memory to train on cpu in float32, more than the 68.7 GB this machine has"
            ),
            "{err}"
        );

        // Qwen3-0.6B.
        let small = Shape::of(
            1_503_300_328,
            &serde_json::json!({"hidden_size": 1024, "intermediate_size": 3072, "num_hidden_layers": 28,
                "vocab_size": 151936, "tie_word_embeddings": true, "torch_dtype": "bfloat16"}),
        )
        .unwrap();
        let training = fit(Device::Mps, &machine, small, 136, "small").unwrap();
        assert_eq!(
            (training.device, training.dtype, training.qlora),
            (Device::Mps, "bfloat16", false)
        );
        assert_eq!(
            small.needs(2, false, true, 136, MPS_RUNTIME)
                - small.needs(2, false, true, 136, RUNTIME),
            MPS_RUNTIME - RUNTIME
        );
        let training = fit(Device::Cpu, &machine, small, 136, "small").unwrap();
        assert_eq!((training.device, training.dtype), (Device::Cpu, "float32"));
    }
}
