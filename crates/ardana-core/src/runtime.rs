//! The runtime abstraction: a [`Runtime`] loads weights into a [`Loaded`] model of one of two kinds, a
//! [`LoadedModel`] (a causal LM that returns label logits at answer slots) or a [`DecisionModel`] (an encoder with a
//! decision head that returns one probability vector per question sequence). A new runtime is one new implementation
//! registered in [`Runtimes`]; nothing above this layer changes.

use std::path::Path;

use crate::engine::DecisionSequence;

/// How to load a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadOptions {
    /// The context window in tokens: the longest prompt row the model accepts.
    pub n_ctx: u32,
    /// Layers offloaded to the GPU: -1 offloads all, 0 runs on the CPU.
    pub gpu_layers: i32,
}

impl Default for LoadOptions {
    /// decider's GGUF engine defaults: a 40,960-token window (a 32,768-token state plus 8k for the question) and every
    /// layer offloaded.
    fn default() -> Self {
        LoadOptions {
            n_ctx: 40_960,
            gpu_layers: -1,
        }
    }
}

pub trait Runtime: Send + Sync {
    /// A stable identifier, e.g. `llama.cpp`.
    fn id(&self) -> &'static str;
    /// Whether this runtime can load the weights at `weights`, a weights file or a [`crate::snapshot`] directory (by
    /// format or by `config.json#architectures`, not by trying).
    fn supports(&self, weights: &Path) -> bool;
    fn load(&self, weights: &Path, opts: &LoadOptions) -> anyhow::Result<Loaded>;
}

pub trait LoadedModel: Send {
    /// The context window the model was loaded with.
    fn n_ctx(&self) -> usize;
    /// Decodes one prompt in isolation and returns, for each slot position in `slots`, one logit per id in
    /// `label_ids`, in `label_ids` order.
    fn slot_logits(
        &mut self,
        ids: &[u32],
        slots: &[usize],
        label_ids: &[u32],
    ) -> anyhow::Result<Vec<Vec<f32>>>;
}

/// A decision model: one forward pass per question sequence, read at its option markers.
pub trait DecisionModel: Send {
    /// One calibrated probability vector per sequence, in marker order: the model applies its own bucket temperatures
    /// before the softmax, so the engine assembles the answers from these as they are.
    fn decide(&mut self, sequences: &[DecisionSequence]) -> anyhow::Result<Vec<Vec<f64>>>;
}

/// What [`Runtime::load`] returns: a model of either kind, answered by [`crate::Decider::answer`].
pub enum Loaded {
    Causal(Box<dyn LoadedModel>),
    Decision(Box<dyn DecisionModel>),
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Loaded::Causal(_) => "Loaded::Causal",
            Loaded::Decision(_) => "Loaded::Decision",
        })
    }
}

/// Every runtime the binary was built with, in preference order.
pub struct Runtimes(pub Vec<Box<dyn Runtime>>);

impl Runtimes {
    pub fn get(&self, id: &str) -> Option<&dyn Runtime> {
        self.0.iter().find(|r| r.id() == id).map(|r| r.as_ref())
    }

    /// The first runtime that supports `weights`.
    pub fn for_weights(&self, weights: &Path) -> Option<&dyn Runtime> {
        self.0
            .iter()
            .find(|r| r.supports(weights))
            .map(|r| r.as_ref())
    }
}

impl std::fmt::Debug for Runtimes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|r| r.id()))
            .finish()
    }
}
