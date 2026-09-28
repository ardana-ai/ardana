//! Prompt building, tokenization, readout and the runtime traits every model runtime implements.
//!
//! The readout reproduces decider 1.6.0 (`Mapika/decider@23579f7`): [`prompt`] builds its token rows, [`systemone`]
//! validates Jev questions and turns probabilities into answers, [`engine::Decider`] ties them to a
//! [`runtime::LoadedModel`], and [`py`] holds the Python semantics (JSON text, rounding, summation) the output
//! depends on.

pub mod engine;
pub mod profile;
pub mod prompt;
pub mod py;
pub mod runtime;
pub mod systemone;

pub use engine::{DecideError, Decider, Limits, Plan};
pub use profile::{AnswerType, Layout, ModelProfile, ProfileError, from_decider_config};
pub use runtime::{LoadOptions, LoadedModel, Runtime, Runtimes};
