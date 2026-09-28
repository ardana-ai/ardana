//! Jev requests on top of the decider prompt format, as `decider/systemone.py` renders, plans and reads them.

use ardana_api::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer};
use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::profile::AnswerType;
use crate::py;

pub const MAX_CHOICE: usize = 255;
pub const MAX_LEVELS: usize = 10;
/// Arrays at least this long get each element's position written into it.
pub const ANNOTATE_MIN: usize = 8;
/// The question text of a noul question without instructions; the options carry what is asked.
pub const NOUL_WITHOUT_INSTRUCTIONS: &str = "Which answer fits the context?";

/// `_txt`: a string as itself, any other JSON value serialised.
fn txt(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py::dumps(other, false),
    }
}

/// `annotate_indices`: every element of an array with at least [`ANNOTATE_MIN`] elements gets its position, as an
/// `_index` key in front of an object's keys or as `{"_index": i, "value": v}`.
pub fn annotate_indices(x: &Value) -> Value {
    match x {
        Value::Array(items) if items.len() >= ANNOTATE_MIN => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let mut out = Map::new();
                    out.insert("_index".into(), Value::from(i));
                    match annotate_indices(v) {
                        Value::Object(fields) => out.extend(fields),
                        other => {
                            out.insert("value".into(), other);
                        }
                    }
                    Value::Object(out)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(annotate_indices).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), annotate_indices(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `render_state`: a string as itself, any other state serialised with long arrays indexed.
pub fn render_state(state: &Value) -> String {
    match state {
        Value::String(s) => s.clone(),
        other => py::dumps(&annotate_indices(other), false),
    }
}

/// What a rendered question reports for each option.
#[derive(Debug, Clone, PartialEq)]
pub enum Names {
    /// Choice option names.
    Choice(Vec<String>),
    /// Score levels `0..n`.
    Score(usize),
    /// Noul: false, true.
    Noul,
}

/// `render_question`'s result.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedQuestion {
    pub question: String,
    pub options: Vec<String>,
    pub kind: AnswerType,
    pub names: Names,
    /// Score only: each level's description.
    pub legend: Vec<String>,
    pub isolated: bool,
}

fn get<'a>(spec: &'a Map<String, Value>, key: &str, fallback: &str) -> Option<&'a Value> {
    spec.get(key).or_else(|| spec.get(fallback))
}

fn blank(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null)) || v.and_then(Value::as_str) == Some("")
}

/// `render_question`: validates one question spec with decider's messages and renders its question and options.
pub fn render_question(spec: &Value) -> Result<RenderedQuestion, String> {
    let Value::Object(spec) = spec else {
        return Err(format!(
            "question must be an object, got {}",
            py::repr(spec)
        ));
    };
    let choice = Value::from("choice");
    let t = spec.get("type").unwrap_or(&choice);
    let t_str = t.as_str();
    let crit = get(spec, "criteria", "options").filter(|v| !v.is_null());
    let raw = get(spec, "instructions", "question");
    let is_noul = matches!(t_str, Some("noul" | "bool"));
    let ins = if is_noul && blank(raw) {
        let described = crit.and_then(Value::as_object).is_some_and(|c| {
            [c.get("true"), c.get("false")]
                .into_iter()
                .any(|d| !blank(d))
        });
        if !described && crit.is_none_or(Value::is_object) {
            return Err(
                "noul question without instructions: criteria must describe true or false".into(),
            );
        }
        NOUL_WITHOUT_INSTRUCTIONS.to_string()
    } else {
        raw.map_or_else(String::new, txt)
    };
    if ins.is_empty() {
        return Err("question without instructions".into());
    }
    let isolated = spec.get("isolated").is_none_or(py::truthy);
    let rendered = |options, kind, names, legend| RenderedQuestion {
        question: ins.clone(),
        options,
        kind,
        names,
        legend,
        isolated,
    };
    match t_str {
        Some("choice") => {
            let map: Vec<(String, Option<&Value>)> = match crit {
                Some(Value::Array(items)) => {
                    let mut names: IndexMap<String, ()> = IndexMap::new();
                    for item in items {
                        names.insert(py::str(item), ());
                    }
                    names.into_keys().map(|n| (n, None)).collect()
                }
                Some(Value::Object(map)) => map.iter().map(|(k, v)| (k.clone(), Some(v))).collect(),
                _ => Vec::new(),
            };
            let is_map = matches!(crit, Some(Value::Array(_) | Value::Object(_)));
            if !is_map || !(2..=MAX_CHOICE).contains(&map.len()) {
                return Err(format!("choice criteria: a map of 2..{MAX_CHOICE} options"));
            }
            let options = map
                .iter()
                .map(|(name, desc)| match desc {
                    Some(d) if !blank(Some(d)) => format!("{name}: {}", txt(d)),
                    _ => name.clone(),
                })
                .collect();
            let names = map.into_iter().map(|(n, _)| n).collect();
            Ok(rendered(
                options,
                AnswerType::Choice,
                Names::Choice(names),
                Vec::new(),
            ))
        }
        Some("score") => {
            let levels: Vec<&Value> = match crit {
                Some(Value::Object(map)) => {
                    let mut keyed = Vec::with_capacity(map.len());
                    for (k, v) in map {
                        let key = py::float_from_str(k).ok_or_else(|| {
                            format!("could not convert string to float: {}", py::str_repr(k))
                        })?;
                        keyed.push((key, v));
                    }
                    keyed
                        .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                    keyed.into_iter().map(|(_, v)| v).collect()
                }
                Some(Value::Array(items)) => items.iter().collect(),
                _ => {
                    return Err(format!(
                        "score criteria: an ordered list of 2..{MAX_LEVELS} level descriptions"
                    ));
                }
            };
            if !(2..=MAX_LEVELS).contains(&levels.len()) {
                return Err(format!(
                    "score criteria: an ordered list of 2..{MAX_LEVELS} level descriptions"
                ));
            }
            let legend: Vec<String> = levels.iter().map(|c| txt(c)).collect();
            let options = legend
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{i}: {c}"))
                .collect();
            let n = legend.len();
            Ok(rendered(
                options,
                AnswerType::Score,
                Names::Score(n),
                legend,
            ))
        }
        Some("noul" | "bool") => {
            let empty = Map::new();
            let c = match crit {
                None => &empty,
                Some(Value::Object(map)) => map,
                Some(_) => {
                    return Err("noul criteria: a map of optional true/false descriptions".into());
                }
            };
            let option = |word: &str, desc: Option<&Value>| match desc {
                Some(d) if !blank(Some(d)) => format!("{word}: {}", txt(d)),
                _ => word.to_string(),
            };
            let options = vec![option("no", c.get("false")), option("yes", c.get("true"))];
            Ok(rendered(options, AnswerType::Noul, Names::Noul, Vec::new()))
        }
        _ => Err(format!("unknown question type {}", py::repr(t))),
    }
}

/// The isolated-level question: the level is judged alone, without its number or its neighbours.
pub fn isolated_question(question: &str, level: &str) -> String {
    format!(
        "{question}\nProposed answer: {}\nDoes the proposed answer fit?",
        strip_level_number(level)
    )
}

/// `strip_level_number`: `"2: somewhat"` -> `"somewhat"` (the regex `^\s*-?\d+\s*:\s*`).
pub fn strip_level_number(text: &str) -> &str {
    let rest = text.trim_start_matches(py::is_space);
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    let after_digits = rest.trim_start_matches(|c: char| c.is_numeric());
    if after_digits.len() == rest.len() {
        return text;
    }
    let after_space = after_digits.trim_start_matches(py::is_space);
    match after_space.strip_prefix(':') {
        Some(tail) => tail.trim_start_matches(py::is_space),
        None => text,
    }
}

/// How a question maps onto rows (`plan_rows`' index entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// One row with all options.
    List,
    /// One yes/no row per score level.
    Iso,
}

impl RowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RowKind::List => "list",
            RowKind::Iso => "iso",
        }
    }
}

/// One question's rows: `first..first + count` in the planned row order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSpan {
    pub id: String,
    pub kind: RowKind,
    pub first: usize,
    pub count: usize,
}

/// One planned scoring row: its question text, options and answer type.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRow {
    pub question: String,
    pub options: Vec<String>,
    pub kind: AnswerType,
}

/// `plan_rows` (plus `row_types`): one row per question, one yes/no row per level of a score question that reads
/// isolated levels.
pub fn plan_rows(
    rqs: &IndexMap<String, RenderedQuestion>,
    isolated: bool,
) -> (Vec<PlannedRow>, Vec<RowSpan>) {
    let (mut rows, mut index) = (Vec::new(), Vec::new());
    for (id, r) in rqs {
        let first = rows.len();
        if isolated && r.kind == AnswerType::Score && r.isolated {
            for level in &r.legend {
                rows.push(PlannedRow {
                    question: isolated_question(&r.question, level),
                    options: vec!["no".into(), "yes".into()],
                    kind: r.kind,
                });
            }
            index.push(RowSpan {
                id: id.clone(),
                kind: RowKind::Iso,
                first,
                count: r.legend.len(),
            });
        } else {
            rows.push(PlannedRow {
                question: r.question.clone(),
                options: r.options.clone(),
                kind: r.kind,
            });
            index.push(RowSpan {
                id: id.clone(),
                kind: RowKind::List,
                first,
                count: 1,
            });
        }
    }
    (rows, index)
}

/// 1 minus the normalised entropy.
pub fn certainty(p: &[f64]) -> f64 {
    if p.len() <= 1 {
        return 1.0;
    }
    let h = -py::sum(p.iter().filter(|&&x| x > 0.0).map(|&x| x * x.ln()));
    (1.0 - h / (p.len() as f64).ln()).max(0.0)
}

/// `min(1.0, max(0.0, x))`, which also maps NaN to 0.
fn clip01(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) }
}

/// `_normalised`: a zero total counts as uniform.
fn normalised(p: &[f64]) -> Vec<f64> {
    let tot = py::sum(p.iter().copied());
    if tot == 0.0 {
        vec![1.0 / p.len() as f64; p.len()]
    } else {
        p.iter().map(|x| x / tot).collect()
    }
}

/// Index of the first largest value (`max(range(n), key=p.__getitem__)`).
fn argmax(p: &[f64]) -> usize {
    let mut best = 0;
    for (i, &x) in p.iter().enumerate() {
        if x > p[best] {
            best = i;
        }
    }
    best
}

/// TypeSafe's choice confidence: `(n * p_max - 1) / (n - 1)`, 1 for a single option.
pub fn choice_confidence(p: &[f64]) -> f64 {
    let n = p.len();
    if n <= 1 {
        return 1.0;
    }
    let p = normalised(p);
    let max = p[argmax(&p)];
    clip01((n as f64 * max - 1.0) / (n as f64 - 1.0))
}

/// TypeSafe's score confidence: 1 minus the expected distance from the most likely level over the mean distance of
/// the levels from the middle of the scale.
pub fn score_confidence(p: &[f64]) -> f64 {
    let n = p.len();
    if n <= 1 {
        return 1.0;
    }
    let p = normalised(p);
    let k = argmax(&p);
    let spread = py::sum(
        p.iter()
            .enumerate()
            .map(|(i, x)| x * (i as i64 - k as i64).abs() as f64),
    );
    let mid = (n as f64 - 1.0) / 2.0;
    let uniform = py::sum((0..n).map(|i| (i as f64 - mid).abs())) / n as f64;
    clip01(1.0 - spread / uniform)
}

/// `format_answer`: probabilities in option order -> the typed answer with decider's rounding, extras `x_`-prefixed.
pub fn format_answer(rq: &RenderedQuestion, p: &[f64]) -> Answer {
    let p = &p[..rq.options.len().min(p.len())];
    let s = py::sum(p.iter().copied());
    let s = if s == 0.0 { 1.0 } else { s };
    let p: Vec<f64> = p.iter().map(|x| x / s).collect();
    let j = argmax(&p);
    let r4 = |x: f64| py::round(x, 4);
    match &rq.names {
        Names::Noul => Answer::Noul(NoulAnswer { noul: r4(p[1]) }),
        Names::Choice(names) => Answer::Choice(ChoiceAnswer {
            choice: names[j].clone(),
            confidence: r4(choice_confidence(&p)),
            probabilities: names
                .iter()
                .cloned()
                .zip(p.iter().map(|&x| r4(x)))
                .collect(),
            x_p_max: Some(r4(p[j])),
            x_certainty: Some(r4(certainty(&p))),
        }),
        Names::Score(_) => Answer::Score(ScoreAnswer {
            score: py::round(py::sum(p.iter().enumerate().map(|(i, x)| i as f64 * x)), 2),
            confidence: r4(score_confidence(&p)),
            legend: rq
                .legend
                .iter()
                .enumerate()
                .map(|(i, d)| (i.to_string(), d.clone()))
                .collect(),
            probabilities: p
                .iter()
                .enumerate()
                .map(|(i, &x)| (i.to_string(), r4(x)))
                .collect(),
            x_p_max: Some(r4(p[j])),
            x_certainty: Some(r4(certainty(&p))),
            x_level_fit: None,
            x_fit_mass: None,
        }),
    }
}

/// `assemble`: one probability list per planned row -> the answers in request order. Isolated levels are combined
/// into one distribution, with each level's own fit and the fit mass as extras.
pub fn assemble(
    rqs: &IndexMap<String, RenderedQuestion>,
    index: &[RowSpan],
    probs: &[Vec<f64>],
) -> IndexMap<String, Answer> {
    let mut out = IndexMap::new();
    for span in index {
        let rq = &rqs[&span.id];
        let answer = match span.kind {
            RowKind::List => format_answer(rq, &probs[span.first]),
            RowKind::Iso => {
                let fit: Vec<f64> = (0..span.count).map(|j| probs[span.first + j][1]).collect();
                let tot = py::sum(fit.iter().copied());
                let tot = if tot == 0.0 { 1e-9 } else { tot };
                let p: Vec<f64> = fit.iter().map(|x| x / tot).collect();
                let mut answer = format_answer(rq, &p);
                if let Answer::Score(score) = &mut answer {
                    score.x_level_fit = Some(
                        fit.iter()
                            .enumerate()
                            .map(|(j, &x)| (j.to_string(), py::round(x, 4)))
                            .collect(),
                    );
                    score.x_fit_mass = Some(py::round(tot, 4));
                }
                answer
            }
        };
        out.insert(span.id.clone(), answer);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_level_number_like_the_regex() {
        assert_eq!(strip_level_number("2: somewhat"), "somewhat");
        assert_eq!(strip_level_number("-1 : negative"), "negative");
        assert_eq!(strip_level_number("no number"), "no number");
        assert_eq!(strip_level_number("12 no colon"), "12 no colon");
    }

    #[test]
    fn render_state_indexes_long_arrays() {
        assert_eq!(render_state(&json!("plain text")), "plain text");
        assert_eq!(
            render_state(&json!({"items": [{"a": 1}, {"a": 2}]})),
            r#"{"items": [{"a": 1}, {"a": 2}]}"#
        );
        let long: Value =
            serde_json::from_str(&render_state(&json!((0..9).collect::<Vec<_>>()))).unwrap();
        assert_eq!(long[4], json!({"_index": 4, "value": 4}));
    }

    #[test]
    fn render_state_reads_floats_like_python() {
        let body = r#"{"amount": 123456789.123456789123456789, "tiny": 2.2250738585072011e-308, "ratio": 0.1234567890123456789}"#;
        let state: Value = serde_json::from_str(body).unwrap();
        // CPython 3.12: `json.dumps(json.loads(body), ensure_ascii=False)`.
        assert_eq!(
            render_state(&state),
            r#"{"amount": 123456789.12345679, "tiny": 2.225073858507201e-308, "ratio": 0.12345678901234568}"#
        );
    }

    #[test]
    fn choice_names_and_descriptions() {
        let rq = render_question(&json!({"type": "choice", "instructions": "Which team?",
            "criteria": {"billing": "charges", "returns": {"what": "refunds", "not_for": "delivery"}, "other": null}}))
        .unwrap();
        assert_eq!(
            rq.options,
            [
                "billing: charges",
                r#"returns: {"what": "refunds", "not_for": "delivery"}"#,
                "other"
            ]
        );
    }
}
