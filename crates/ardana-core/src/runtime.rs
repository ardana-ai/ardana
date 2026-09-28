//! The runtime abstraction: a [`Runtime`] loads weights into a [`LoadedModel`] that returns label logits at answer
//! slots. A new runtime is one new implementation registered in [`Runtimes`]; nothing above this layer changes.

use std::path::Path;

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
    /// Whether this runtime can load the weights at `weights` (by format, not by trying).
    fn supports(&self, weights: &Path) -> bool;
    fn load(&self, weights: &Path, opts: &LoadOptions) -> anyhow::Result<Box<dyn LoadedModel>>;
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
