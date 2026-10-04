//! Jev wire types shared by the server and the playground; compiles for `wasm32-unknown-unknown`.
//!
//! The request keeps every question spec as raw JSON so the server can answer a malformed spec with decider's own
//! validation message instead of a deserialisation error; [`Question`] is the typed form clients build specs with.
//! Maps keep insertion order, so answers follow the request's question order. [`error`] holds the error bodies.

pub mod error;
pub mod format;
pub mod library;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use error::{Detail, ErrorBody, ValidationItem};
pub use library::{
    BrowserEntry, LIBRARY_SCHEMA, LayoutKind, LibraryDocument, LibraryEntry, LibraryError,
    LibraryManifest, LibraryTag,
};

/// A request's `state` from text as a person typed it: text that parses as a JSON object or array is sent as that
/// JSON; any other text is sent as a string. The playground and `ardana run` read the state this way.
pub fn state_value(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(value @ (Value::Object(_) | Value::Array(_))) => value,
        _ => Value::String(text.to_string()),
    }
}

/// `POST /v1/systemone` request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneRequest {
    /// A configured model name, `jev-latest` or any `jev-*` name; `None` picks the default model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Plain text, or any JSON value (serialised compactly, long arrays indexed).
    pub state: Value,
    /// Question id to question spec, in request order. Each spec is normally a [`Question`] object.
    #[serde(default)]
    pub questions: IndexMap<String, Value>,
}

/// The three Jev question types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    Choice,
    Score,
    Noul,
}

/// A typed question spec, as clients write it into [`SystemOneRequest::questions`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    #[serde(rename = "type")]
    pub kind: QuestionType,
    /// A string or any JSON value; optional for `noul` when `criteria` describes true or false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Value>,
    /// `choice`: a map of option name to description (or null), or a list of names. `score`: a list of 2..10 level
    /// descriptions, or a `{"0": .., "1": ..}` legend. `noul`: optional `{"true": .., "false": ..}` descriptions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<Value>,
    /// `score` only: `false` reads the levels listwise instead of one yes/no row per level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated: Option<bool>,
}

/// `POST /v1/systemone` response body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub model: String,
    /// Question id to answer, in request order.
    pub answers: IndexMap<String, Answer>,
    pub usage: Usage,
}

/// One typed answer; serialises with `type` first, then the official keys, then `x_` extras.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
    Noul(NoulAnswer),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    /// The most probable option name.
    pub choice: String,
    /// TypeSafe's choice confidence, `(n * p_max - 1) / (n - 1)`.
    pub confidence: f64,
    /// Option name to probability, in option order.
    pub probabilities: IndexMap<String, f64>,
    /// The largest probability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_p_max: Option<f64>,
    /// One minus the normalised entropy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_certainty: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    /// The expected level, rounded to 2 places.
    pub score: f64,
    /// TypeSafe's score confidence.
    pub confidence: f64,
    /// Level (`"0"`, `"1"`, ...) to its description; never null.
    pub legend: IndexMap<String, String>,
    /// Level to probability.
    pub probabilities: IndexMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_p_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_certainty: Option<f64>,
    /// Isolated levels only: each level's own probability of fitting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_level_fit: Option<IndexMap<String, f64>>,
    /// Isolated levels only: the sum of the level fits before normalising.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_fit_mass: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    /// The probability of "yes".
    pub noul: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Prompt tokens, counting the state shared by every row once.
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// `GET /v1/models` response body.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub description: String,
    pub release_date: String,
    /// Whether the server has pulled the model; a library model it has not is pulled by its first API request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_pulled: Option<bool>,
    /// Whether this is the model requests without a model use.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub x_default: bool,
    /// A library model not pulled yet: its download size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_size: Option<u64>,
    /// A model with a browser variant (ONNX weights a visitor's tab runs): the bytes the tab downloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_browser: Option<u64>,
    /// Whether the server holds the browser variant whole in its Hugging Face cache, so it answers a tab's request for
    /// the files at once; otherwise the first request pulls them from the Hub first. In the standalone playground's
    /// list, every browser variant: the Hub holds it whole.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub x_browser_pulled: bool,
    /// Whether this is the browser model a playground offers first (the library's `browser_default`): its tab row is
    /// where the picker opens while the default model is not pulled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub x_browser_default: bool,
}

impl ModelInfo {
    /// Whether the model is pulled; a list without `x_pulled` lists pulled models only.
    pub fn pulled(&self) -> bool {
        self.x_pulled != Some(false)
    }
}

/// A byte count in decimal units, as Ollama prints model sizes (`2.0 GB`): model sizes in the CLI, in download
/// progress and, from `x_size`, in the playground.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_decimal() {
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(2_019_377_376), "2.0 GB");
        assert_eq!(human_size(563_000_000), "563.0 MB");
        assert_eq!(human_size(1_274_396_800), "1.3 GB");
    }

    #[test]
    fn answers_serialise_type_first_and_skip_absent_extras() {
        let answer = Answer::Noul(NoulAnswer { noul: 0.95 });
        assert_eq!(
            serde_json::to_string(&answer).unwrap(),
            r#"{"type":"noul","noul":0.95}"#
        );
        let choice = Answer::Choice(ChoiceAnswer {
            choice: "a".into(),
            confidence: 1.0,
            probabilities: IndexMap::from([("a".to_string(), 1.0), ("b".to_string(), 0.0)]),
            x_p_max: Some(1.0),
            x_certainty: None,
        });
        assert_eq!(
            serde_json::to_string(&choice).unwrap(),
            r#"{"type":"choice","choice":"a","confidence":1.0,"probabilities":{"a":1.0,"b":0.0},"x_p_max":1.0}"#
        );
    }

    #[test]
    fn request_keeps_question_order_and_raw_specs() {
        let req: SystemOneRequest = serde_json::from_str(
            r#"{"state":"s","questions":{"z":{"type":"noul","instructions":"q"},"a":"not an object"}}"#,
        )
        .unwrap();
        assert_eq!(req.questions.keys().collect::<Vec<_>>(), ["z", "a"]);
        assert_eq!(req.questions["a"], Value::String("not an object".into()));
    }
}
