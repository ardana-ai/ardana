//! What a model needs besides its weights and tokenizer: prompt layout, answer temperatures, level isolation and
//! naming, read from decider's `decider_config.json` (`decider/temperature.py#from_config`, `serve.py#apply_config`);
//! for a decision model (THX-01), its sequence limits and base temperatures, read from the `thx01` package's
//! `rl_agent_config.json` ([`from_rl_agent_config`]).

use std::collections::BTreeMap;
use std::fmt;

use ardana_api::LayoutKind;
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
    /// A decision model's sequence limits; `None` for a causal model, which reads prompt rows in `layout`. Absent from
    /// the `models.toml` entries written before decision models existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionFormat>,
}

/// How a decision model's question sequences are built and bounded (`thx01.common.build_sequence`,
/// `Agent._head_budget`):
/// known at pull time from `rl_agent_config.json`, before any load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionFormat {
    /// `max_len`: the tokens one sequence (question, options and state) may hold; a longer one is refused.
    pub window: usize,
    /// `tournament_above`: the options a choice question may have; above it the package runs a tournament, which
    /// Ardana refuses instead.
    pub max_options: usize,
    /// `head_max_len`: the tokens of the question and options together, raised to what the options need up to half
    /// the window.
    pub head_max_len: usize,
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
            decision: None,
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
        decision: None,
    })
}

/// The `thx01` package's defaults for an `rl_agent_config.json` that leaves a key out (`Agent.system_one`,
/// `Agent._head_budget`, `predict_batch`).
const THX01_MAX_LEN: usize = 512;
const THX01_HEAD_MAX_LEN: usize = 192;
const THX01_TOURNAMENT_ABOVE: usize = 24;
/// `thx01.common.TEMP_MIN` and `TEMP_MAX`: the range a temperature is confined to before it is applied.
pub const THX01_TEMPERATURE_RANGE: (f64, f64) = (0.5, 5.0);
/// The package's `QTYPES` order of the `temperature` triple: choice, score, noul.
const THX01_TYPES: [AnswerType; 3] = [AnswerType::Choice, AnswerType::Score, AnswerType::Noul];

/// `thx01.common.clamp_temperature`: `t` confined to [0.5, 5.0]; 1.0 when it is no finite number.
pub fn clamp_temperature(t: &Value) -> f64 {
    let (lo, hi) = THX01_TEMPERATURE_RANGE;
    match t {
        Value::Number(n) => n
            .as_f64()
            .filter(|t| t.is_finite())
            .map_or(1.0, |t| t.clamp(lo, hi)),
        Value::String(s) => py::float_from_str(s)
            .filter(|t| t.is_finite())
            .map_or(1.0, |t| t.clamp(lo, hi)),
        _ => 1.0,
    }
}

/// The profile of a THX-01 decision model from the `thx01` package's `rl_agent_config.json`: named `model_name`
/// (`THX-01` without one, the `model` the package reports), its limits from `max_len`, `tournament_above` (`null` or
/// 0, no tournament, allows every width the API does) and `head_max_len`, and `temperature_by_type` the base triple
/// `temperature` (choice, score, noul) clamped as the package clamps it; the per-bucket temperatures the model applies
/// stay with the runtime. `auto_head_budget: false` is refused: the head budget is built as the package builds it with
/// the flag on, which the published checkpoint writes.
pub fn from_rl_agent_config(cfg: &Value) -> Result<ModelProfile, ProfileError> {
    const WHERE: &str = "rl_agent_config.json";
    let Value::Object(cfg) = cfg else {
        return Err(ProfileError::Invalid(format!(
            "{WHERE} must be a JSON object, got {}",
            py::repr(cfg)
        )));
    };
    let count = |key: &str, default: usize| -> Result<Option<usize>, ProfileError> {
        match cfg.get(key) {
            None => Ok(Some(default)),
            Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => Ok(n.as_u64().map(|n| n as usize).filter(|&n| n > 0)),
            Some(other) => Err(ProfileError::Invalid(format!(
                "{WHERE} \"{key}\" must be a positive integer, got {}",
                py::repr(other)
            ))),
        }
    };
    let positive = |key: &str, default: usize| -> Result<usize, ProfileError> {
        count(key, default)?.ok_or_else(|| {
            ProfileError::Invalid(format!("{WHERE} \"{key}\" must be a positive integer"))
        })
    };
    if cfg.get("auto_head_budget").is_some_and(|v| !py::truthy(v)) {
        return Err(ProfileError::Invalid(format!(
            "{WHERE} turns \"auto_head_budget\" off, and only the package's default head budget is implemented"
        )));
    }
    let decision = DecisionFormat {
        window: positive("max_len", THX01_MAX_LEN)?,
        max_options: count("tournament_above", THX01_TOURNAMENT_ABOVE)?
            .unwrap_or(crate::systemone::MAX_CHOICE),
        head_max_len: positive("head_max_len", THX01_HEAD_MAX_LEN)?,
    };
    let temperatures = match cfg.get("temperature") {
        None | Some(Value::Null) => vec![Value::from(1.0); 3],
        Some(Value::Array(items)) if items.len() == 3 => items.clone(),
        Some(other) => {
            return Err(ProfileError::Invalid(format!(
                "{WHERE} \"temperature\" must be a list of three numbers (choice, score, noul), got {}",
                py::repr(other)
            )));
        }
    };
    let temperature_by_type = THX01_TYPES
        .into_iter()
        .zip(temperatures.iter().map(clamp_temperature))
        .collect();
    Ok(ModelProfile {
        name: cfg
            .get("model_name")
            .and_then(Value::as_str)
            .unwrap_or("THX-01")
            .to_string(),
        layout: Layout::Plain,
        temperature: 1.0,
        temperature_by_type,
        isolated_levels: false,
        release_date: None,
        decision: Some(decision),
    })
}

/// The layout a `decider_config.json` names, which a model pulled without `--layout` is read in: plain unless its
/// `"layout"` or `"chat_template": true` says chat ([`resolve_layout`]). A config that is no object names none.
pub fn decider_layout(cfg: &Value) -> Result<LayoutKind, ProfileError> {
    let named = match cfg {
        Value::Object(map) => resolve_layout(map)?,
        _ => "plain",
    };
    Ok(if named == "chat" {
        LayoutKind::Chat
    } else {
        LayoutKind::Plain
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
            decider_layout(&json!({"layout": "chat"})).unwrap(),
            LayoutKind::Chat
        );
        assert_eq!(
            decider_layout(&json!({"chat_template": true})).unwrap(),
            LayoutKind::Chat
        );
        assert_eq!(decider_layout(&json!({})).unwrap(), LayoutKind::Plain);
        assert!(decider_layout(&json!({"layout": "xml_v2"})).is_err());
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
        assert_eq!(p.decision, None);
    }

    /// THX-01's `rl_agent_config.json`: the limits, the clamped base triple in the package's order (choice, score,
    /// noul), the name, and the defaults of a config that leaves the keys out; a tournament limit of `null` lets a
    /// choice question be as wide as the API allows, `auto_head_budget: false` is refused, and an entry written
    /// without `decision` still reads.
    #[test]
    fn reads_thx01_config() {
        let cfg = json!({"model_name": "THX-01", "max_len": 1024, "head_max_len": 256, "tournament_above": 24,
                         "temperature": [2.55, 3.05, 0.1], "auto_head_budget": true,
                         "temperature_by_options": {"noul:2": 2.0}});
        let p = from_rl_agent_config(&cfg).unwrap();
        assert_eq!(p.name, "THX-01");
        assert_eq!(
            p.decision,
            Some(DecisionFormat {
                window: 1024,
                max_options: 24,
                head_max_len: 256
            })
        );
        assert_eq!(p.temperature_for(AnswerType::Choice), 2.55);
        assert_eq!(p.temperature_for(AnswerType::Score), 3.05);
        assert_eq!(p.temperature_for(AnswerType::Noul), 0.5);
        assert!(!p.isolated_levels);

        let p = from_rl_agent_config(&json!({"temperature": ["7", null, 1.5]})).unwrap();
        assert_eq!(p.name, "THX-01");
        assert_eq!(
            p.decision,
            Some(DecisionFormat {
                window: 512,
                max_options: 24,
                head_max_len: 192
            })
        );
        assert_eq!(p.temperature_for(AnswerType::Choice), 5.0);
        assert_eq!(p.temperature_for(AnswerType::Score), 1.0);
        assert_eq!(p.temperature_for(AnswerType::Noul), 1.5);
        let wide = from_rl_agent_config(&json!({"tournament_above": null})).unwrap();
        assert_eq!(wide.decision.unwrap().max_options, 255);
        let err = from_rl_agent_config(&json!({"auto_head_budget": false})).unwrap_err();
        assert!(err.to_string().contains("auto_head_budget"), "{err}");
        let err = from_rl_agent_config(&json!({"max_len": "long"})).unwrap_err();
        assert!(err.to_string().contains("max_len"), "{err}");

        let written: ModelProfile =
            serde_json::from_str(r#"{"name":"m","layout":{"kind":"plain"},"temperature":1.0,"temperature_by_type":{},"isolated_levels":false,"release_date":null}"#)
                .unwrap();
        assert_eq!(written.decision, None);
        assert!(
            !serde_json::to_string(&written)
                .unwrap()
                .contains("decision")
        );
        assert!(
            serde_json::to_string(&p)
                .unwrap()
                .contains("\"window\":512")
        );
    }
}
