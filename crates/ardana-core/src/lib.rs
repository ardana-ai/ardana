//! Prompt building, tokenization, readout and the runtime traits every model runtime implements.
//!
//! The readout reproduces decider 1.6.0 (`Mapika/decider@23579f7`): [`prompt`] builds its token rows, [`systemone`]
//! validates Jev questions and turns probabilities into answers, [`engine::Decider`] ties them to a
//! [`runtime::LoadedModel`], and [`py`] holds the Python semantics (JSON text, rounding, summation) the output
//! depends on. [`chat`] derives the chat layout of stock instruct models from their chat template.

pub mod chat;
pub mod engine;
pub mod profile;
pub mod prompt;
pub mod py;
pub mod runtime;
pub mod systemone;

/// For the crates that may depend on the core only (`ardana-registry`): the size format and the library document's
/// types (C3), which the registry reads at run time.
pub use ardana_api::{
    BrowserEntry, GgufEntry, GgufQuant, LayoutKind, LibraryDocument, LibraryError, LibraryFamily,
    LibraryManifest, LibrarySize, RepoPin, human_size,
};
pub use chat::{ChatTemplateError, TemplateSpecials, chat_layout, read_template};
pub use engine::{DecideError, Decider, Decode, Limits, Plan};
pub use profile::{AnswerType, Layout, ModelProfile, ProfileError, from_decider_config};
pub use runtime::{LoadOptions, LoadedModel, Runtime, Runtimes};
/// The tokenizer a [`Decider`] reads with, for the playground, which builds one from `tokenizer.json`'s bytes on its
/// own regex backend (`docs/guidelines/huggingface.md`, "Tokenizers").
pub use tokenizers::Tokenizer;
