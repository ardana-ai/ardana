//! What a model needs besides its weights and tokenizer: prompt layout, answer temperatures, level isolation and
//! naming, read from decider's `decider_config.json` (`decider/temperature.py#from_config`, `serve.py#apply_config`).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::py;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    /// The `model` every response reports (decider: `"decider-" + version`).
    pub name: String,
    pub layout: Layout,
    /// The temperature of every answer type without its own entry.
    pub temperature: f64,
    pub temperature_by_type: BTreeMap<AnswerType, f64>,
    /// Score questions read one yes/no row per level unless the question says `"isolated": false`.
    pub isolated_levels: bool,
    pub release_date: Option<String>,
}

/// How a prompt row is wrapped.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Layout {
    /// decider's plain state-first layout, which every decider-format model is trained on.
    #[default]
    Plain,
    /// The tokenizer's chat template around one user turn: `head` opens it, `tail` closes it and opens the assistant turn.
    Chat { head: Vec<u32>, tail: Vec<u32> },
}

/// The answer types temperatures are keyed by (`choice`, `noul`, `score`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerType {
    Choice,
    Noul,
    Score,
}

impl AnswerType {
    pub const ALL: [AnswerType; 3] = [AnswerType::Choice, AnswerType::Noul, AnswerType::Score];

    pub fn as_str(self) -> &'static str {
        match self {
            AnswerType::Choice => "choice",
            AnswerType::Noul => "noul",
            AnswerType::Score => "score",
        }
    }

    fn parse(key: &str) -> Option<AnswerType> {
        AnswerType::ALL.into_iter().find(|t| t.as_str() == key)
    }
}

impl fmt::Display for AnswerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `decider_config.json` Ardana refuses, with decider's message where decider has one.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProfileError {
    #[error("{0}")]
    Invalid(String),
}

const WHERE: &str = "decider_config.json";
const KEYS_TEXT: &str = "the keys are \"choice\", \"noul\" and \"score\" (a /decide \"bool\" field is \"noul\", a \"scale\" field is \"score\"; a /v1/systemone \"bool\" question is \"noul\")";

impl ModelProfile {
    /// The value an answer of type `t` is divided by before the softmax.
    pub fn temperature_for(&self, t: AnswerType) -> f64 {
        self.temperature_by_type
            .get(&t)
            .copied()
            .unwrap_or(self.temperature)
    }

    /// The temperature every answer type gets (`temperature.effective`), for reporting.
    pub fn effective_temperatures(&self) -> BTreeMap<AnswerType, f64> {
        AnswerType::ALL
            .into_iter()
            .map(|t| (t, self.temperature_for(t)))
            .collect()
    }

    /// The defaults of a model without `decider_config.json` (decider `serve.py#apply_config({})`): temperature 1.0,
    /// no per-type temperatures, listwise score levels.
    pub fn stock(name: &str, layout: Layout) -> ModelProfile {
        ModelProfile {
            name: name.to_string(),
            layout,
            temperature: 1.0,
            temperature_by_type: BTreeMap::new(),
            isolated_levels: false,
            release_date: None,
        }
    }
}

/// The profile of a decider-format model from its `decider_config.json`. `layout` is the layout the caller derived;
/// a config naming another layout (or one decider does not know) is refused.
pub fn from_decider_config(cfg: &Value, layout: Layout) -> Result<ModelProfile, ProfileError> {
    let empty = serde_json::Map::new();
    let cfg = match cfg {
        Value::Object(map) => map,
        Value::Null => &empty,
        other => {
            return Err(ProfileError::Invalid(format!(
                "{WHERE} must be a JSON object, got {}",
                py::repr(other)
            )));
        }
    };
    let named = resolve_layout(cfg)?;
    let given = match layout {
        Layout::Plain => "plain",
        Layout::Chat { .. } => "chat",
    };
    if named != given {
        return Err(ProfileError::Invalid(format!(
            "{WHERE} names the prompt layout '{named}', but the model is read in the {given} layout"
        )));
    }
    let temperature = positive(
        cfg.get("temperature").unwrap_or(&Value::from(1.0)),
        &format!("{WHERE} \"temperature\""),
    )?;
    let temperature_by_type = by_type(
        cfg.get("temperature_by_type"),
        &format!("{WHERE} \"temperature_by_type\""),
    )?;
    let version = cfg
        .get("version")
        .map_or_else(|| "dev".to_string(), py::str);
    Ok(ModelProfile {
        name: format!("decider-{version}"),
        layout,
        temperature,
        temperature_by_type,
        isolated_levels: cfg.get("isolated_levels").is_some_and(py::truthy),
        release_date: cfg.get("release_date").map(py::str),
    })
}

/// `decider.prompt.resolve_layout`: `"plain"` unless `"layout"` or `"chat_template": true` says `"chat"`.
fn resolve_layout(cfg: &serde_json::Map<String, Value>) -> Result<&'static str, ProfileError> {
    let chat_template = cfg.get("chat_template") == Some(&Value::Bool(true));
    let layout = match cfg.get("layout") {
        None | Some(Value::Null) => {
            if chat_template {
                "chat"
            } else {
                "plain"
            }
        }
        Some(Value::String(s)) if s == "plain" => "plain",
        Some(Value::String(s)) if s == "chat" => "chat",
        Some(other) => {
            return Err(ProfileError::Invalid(format!(
                "decider_config.json names the prompt layout {}; this version of decider-ai knows 'plain', 'chat'. Upgrade decider-ai, or check the model's config.",
                py::repr(other)
            )));
        }
    };
    if layout == "plain" && chat_template {
        return Err(ProfileError::Invalid(
            "decider_config.json says \"layout\": \"plain\" and \"chat_template\": true; these contradict each other"
                .into(),
        ));
    }
    Ok(layout)
}

/// `temperature.positive`: a number, or a numeric string as Python's `float()` reads it, finite and > 0.
fn positive(value: &Value, place: &str) -> Result<f64, ProfileError> {
    let bad = || {
        ProfileError::Invalid(format!(
            "{place} must be a finite number > 0, got {}",
            py::repr(value)
        ))
    };
    let v = match value {
        Value::Number(n) => n.as_f64().ok_or_else(bad)?,
        Value::String(s) => py::float_from_str(s).ok_or_else(bad)?,
        _ => return Err(bad()),
    };
    if !v.is_finite() || v <= 0.0 {
        return Err(bad());
    }
    Ok(v)
}

/// `temperature.by_type`: a map of answer type to temperature; `null` or absent is empty.
fn by_type(m: Option<&Value>, place: &str) -> Result<BTreeMap<AnswerType, f64>, ProfileError> {
    let map = match m {
        None | Some(Value::Null) => return Ok(BTreeMap::new()),
        Some(Value::Object(map)) => map,
        Some(other) => {
            let kind = match other {
                Value::Bool(_) => "bool",
                Value::Number(n) if n.is_f64() => "float",
                Value::Number(_) => "int",
                Value::String(_) => "str",
                _ => "list",
            };
            return Err(ProfileError::Invalid(format!(
                "{place} must be a map {{answer type: temperature}}, got {kind}; {KEYS_TEXT}"
            )));
        }
    };
    let mut out = BTreeMap::new();
    for (key, v) in map {
        let t = AnswerType::parse(key).ok_or_else(|| {
            ProfileError::Invalid(format!(
                "{place} has the unknown key {}; {KEYS_TEXT}",
                py::str_repr(key)
            ))
        })?;
        let item = format!("{place}[{}]", py::str_repr(key));
        if !v.is_number() {
            return Err(ProfileError::Invalid(format!(
                "{item} must be a finite number > 0, got {}",
                py::repr(v)
            )));
        }
        out.insert(t, positive(v, &item)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_decider_2b_config() {
        let cfg = json!({"temperature": 1.145, "temperature_by_type": {"choice": 1.164, "noul": 1.624, "score": 1.124},
                         "version": "2b-v11", "layout": "plain", "isolated_levels": true, "release_date": "2026-09-24"});
        let p = from_decider_config(&cfg, Layout::Plain).unwrap();
        assert_eq!(p.name, "decider-2b-v11");
        assert_eq!(p.temperature_for(AnswerType::Noul), 1.624);
        assert!(p.isolated_levels);
        assert_eq!(p.release_date.as_deref(), Some("2026-09-24"));
    }

    #[test]
    fn refuses_what_decider_refuses() {
        let err = |cfg: Value| {
            from_decider_config(&cfg, Layout::Plain)
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            err(json!({"temperature": 0})),
            "decider_config.json \"temperature\" must be a finite number > 0, got 0"
        );
        assert!(
            err(json!({"temperature_by_type": {"bool": 1.0}}))
                .contains("has the unknown key 'bool'")
        );
        assert!(err(json!({"layout": "xml_v2"})).contains("'xml_v2'"));
        assert!(err(json!({"layout": "chat"})).contains("read in the plain layout"));
        assert_eq!(
            from_decider_config(&json!({"temperature": "1.5"}), Layout::Plain)
                .unwrap()
                .temperature,
            1.5
        );
    }

    #[test]
    fn stock_defaults() {
        let p = ModelProfile::stock("m", Layout::Plain);
        assert_eq!((p.temperature, p.isolated_levels), (1.0, false));
        assert!(p.temperature_by_type.is_empty());
    }
}
