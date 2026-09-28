//! The question builder's edits, applied to the raw `questions` map so every key the builder does not touch (and
//! every form it does not rewrite) stays as the questions JSON has it. Jev's criteria forms: noul `criteria`
//! `{"true"?, "false"?}`; choice `criteria` a map of 2..255 option names to a description or `null` (or a list of
//! names); score `criteria` an ordered list of 2..10 levels, lowest first (or a `{"0": ..}` legend).

use std::collections::HashMap;
use std::ops::RangeInclusive;

use ardana_api::QuestionType;
use serde_json::{Map, Value, json};

use crate::request::Questions;

pub const CHOICE_OPTIONS: RangeInclusive<usize> = 2..=255;
pub const SCORE_LEVELS: RangeInclusive<usize> = 2..=10;
pub const KINDS: [QuestionType; 3] = [
    QuestionType::Noul,
    QuestionType::Choice,
    QuestionType::Score,
];

pub fn kind_name(kind: QuestionType) -> &'static str {
    match kind {
        QuestionType::Noul => "noul",
        QuestionType::Choice => "choice",
        QuestionType::Score => "score",
    }
}

/// A text field of a question as the builder shows it: text it can edit, or JSON structure it leaves to the
/// questions JSON editor (shown compactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Text(String),
    Structured(String),
}

impl Field {
    fn of(value: Option<&Value>) -> Field {
        match value {
            None | Some(Value::Null) => Field::Text(String::new()),
            Some(Value::String(text)) => Field::Text(text.clone()),
            Some(other) => Field::Structured(other.to_string()),
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Field::Text(text) | Field::Structured(text) => text,
        }
    }

    pub fn editable(&self) -> bool {
        matches!(self, Field::Text(_))
    }
}

/// A question's criteria as the builder edits them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Criteria {
    Noul {
        yes: Field,
        no: Field,
    },
    /// Option names and descriptions, in order.
    Choice(Vec<(String, Field)>),
    /// Level descriptions, lowest first.
    Score(Vec<Field>),
    /// Criteria in a shape the builder cannot edit, and why.
    Unreadable(String),
}

pub fn kind(spec: &Value) -> Option<QuestionType> {
    serde_json::from_value(spec.get("type")?.clone()).ok()
}

pub fn instructions(spec: &Value) -> Field {
    Field::of(spec.get("instructions"))
}

pub fn criteria(spec: &Value) -> Criteria {
    let Value::Object(obj) = spec else {
        return Criteria::Unreadable("the question is not a JSON object".into());
    };
    let crit = obj.get("criteria").filter(|c| !c.is_null());
    match (kind(spec), crit) {
        (None, _) => Criteria::Unreadable("the question has no noul, choice or score type".into()),
        (Some(QuestionType::Noul), None) => Criteria::Noul {
            yes: Field::of(None),
            no: Field::of(None),
        },
        (Some(QuestionType::Noul), Some(Value::Object(c))) => Criteria::Noul {
            yes: Field::of(c.get("true")),
            no: Field::of(c.get("false")),
        },
        (Some(QuestionType::Choice), None) => Criteria::Choice(Vec::new()),
        (Some(QuestionType::Choice), Some(Value::Array(names))) => Criteria::Choice(
            names
                .iter()
                .map(|name| (Field::of(Some(name)).text().to_string(), Field::of(None)))
                .collect(),
        ),
        (Some(QuestionType::Choice), Some(Value::Object(map))) => Criteria::Choice(
            map.iter()
                .map(|(name, note)| (name.clone(), Field::of(Some(note))))
                .collect(),
        ),
        (Some(QuestionType::Score), None) => Criteria::Score(Vec::new()),
        (Some(QuestionType::Score), Some(Value::Array(levels))) => {
            Criteria::Score(levels.iter().map(|l| Field::of(Some(l))).collect())
        }
        (Some(QuestionType::Score), Some(Value::Object(legend))) => match legend_levels(legend) {
            Some(levels) => {
                Criteria::Score(levels.into_iter().map(|l| Field::of(Some(l))).collect())
            }
            None => Criteria::Unreadable("the score legend has a key that is not a number".into()),
        },
        (Some(kind), Some(_)) => Criteria::Unreadable(format!(
            "the {} criteria are not in a form the builder edits",
            kind_name(kind)
        )),
    }
}

/// A `{"0": .., "1": ..}` score legend's levels, ordered by their numeric keys.
fn legend_levels(legend: &Map<String, Value>) -> Option<Vec<&Value>> {
    let mut keyed = legend
        .iter()
        .map(|(key, level)| key.trim().parse::<f64>().ok().map(|k| (k, level)))
        .collect::<Option<Vec<_>>>()?;
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(keyed.into_iter().map(|(_, level)| level).collect())
}

/// The question as an object, replacing anything else with an empty one.
fn object(spec: &mut Value) -> &mut Map<String, Value> {
    if !spec.is_object() {
        *spec = Value::Object(Map::new());
    }
    spec.as_object_mut().expect("just made an object")
}

/// Switches the question's type. The option names of a choice become the levels of a score and the reverse;
/// a new choice or score without them gets two blank entries to fill in, and a noul drops its criteria.
pub fn set_kind(spec: &mut Value, new: QuestionType) {
    let old = kind(spec);
    if old == Some(new) {
        return;
    }
    let carried: Vec<String> = match criteria(spec) {
        Criteria::Choice(options) => options.into_iter().map(|(name, _)| name).collect(),
        Criteria::Score(levels) => levels.iter().map(|l| l.text().to_string()).collect(),
        Criteria::Noul { .. } | Criteria::Unreadable(_) => Vec::new(),
    };
    let obj = object(spec);
    obj.insert("type".into(), json!(kind_name(new)));
    match new {
        QuestionType::Noul => {
            obj.remove("criteria");
        }
        QuestionType::Choice => {
            let mut map = Map::new();
            for name in carried {
                if map.len() < *CHOICE_OPTIONS.end() {
                    map.entry(name).or_insert(Value::Null);
                }
            }
            while map.len() < *CHOICE_OPTIONS.start() {
                map.insert(unused_name(map.keys().map(String::as_str)), Value::Null);
            }
            obj.insert("criteria".into(), Value::Object(map));
        }
        QuestionType::Score => {
            let mut levels: Vec<Value> = carried
                .into_iter()
                .take(*SCORE_LEVELS.end())
                .map(Value::String)
                .collect();
            levels.resize(levels.len().max(*SCORE_LEVELS.start()), json!(""));
            obj.insert("criteria".into(), Value::Array(levels));
        }
    }
    if new != QuestionType::Score {
        obj.remove("isolated");
    }
}

/// A question's type-specific keys (`criteria`, `isolated`) per type it has had, so switching its type back
/// restores them.
pub type KindMemory = HashMap<QuestionType, Map<String, Value>>;

const KIND_KEYS: [&str; 2] = ["criteria", "isolated"];

/// Switches the question's type like [`set_kind`], first keeping the old type's criteria in `memory`; a type the
/// question had before gets its own criteria back.
pub fn switch_kind(spec: &mut Value, new: QuestionType, memory: &mut KindMemory) {
    let old = kind(spec);
    if old == Some(new) {
        return;
    }
    if let (Some(old), Value::Object(obj)) = (old, &*spec) {
        let kept = KIND_KEYS
            .iter()
            .filter_map(|key| obj.get(*key).map(|v| (key.to_string(), v.clone())))
            .collect();
        memory.insert(old, kept);
    }
    match memory.get(&new) {
        Some(kept) => {
            let obj = object(spec);
            for key in KIND_KEYS {
                obj.remove(key);
            }
            obj.insert("type".into(), json!(kind_name(new)));
            obj.extend(kept.clone());
        }
        None => set_kind(spec, new),
    }
}

pub fn set_instructions(spec: &mut Value, text: &str) {
    object(spec).insert("instructions".into(), json!(text));
}

/// Sets what a true (`yes`) or false answer means; empty text removes it, and criteria left empty are removed.
pub fn set_noul(spec: &mut Value, yes: bool, text: &str) -> Result<(), String> {
    if !matches!(criteria(spec), Criteria::Noul { .. }) {
        return Err("not a noul question with editable criteria".into());
    }
    let obj = object(spec);
    let key = if yes { "true" } else { "false" };
    let crit = obj.entry("criteria").or_insert(Value::Null);
    if crit.is_null() {
        *crit = json!({});
    }
    let crit = crit.as_object_mut().expect("noul criteria are an object");
    if text.is_empty() {
        crit.remove(key);
    } else {
        crit.insert(key.into(), json!(text));
    }
    if crit.is_empty() {
        obj.remove("criteria");
    }
    Ok(())
}

/// The choice criteria, when the builder can edit them: a list of names or a map of names to descriptions.
fn choice_criteria(spec: &mut Value) -> Result<&mut Value, String> {
    if !matches!(criteria(spec), Criteria::Choice(_)) {
        return Err("not a choice question with editable options".into());
    }
    let crit = object(spec).entry("criteria").or_insert(Value::Null);
    if crit.is_null() {
        *crit = json!({});
    }
    Ok(crit)
}

/// Renames option `index`; the new name must differ from every other option's.
pub fn set_option_name(spec: &mut Value, index: usize, name: &str) -> Result<(), String> {
    let crit = choice_criteria(spec)?;
    let names = option_names(crit);
    if names
        .iter()
        .enumerate()
        .any(|(i, other)| i != index && other == name)
    {
        return Err(format!("another option is already named {name:?}"));
    }
    match crit {
        Value::Array(items) => {
            let item = items.get_mut(index).ok_or("no such option")?;
            *item = json!(name);
        }
        Value::Object(map) => {
            if index >= map.len() {
                return Err("no such option".into());
            }
            *map = std::mem::take(map)
                .into_iter()
                .enumerate()
                .map(|(i, (key, note))| {
                    if i == index {
                        (name.to_string(), note)
                    } else {
                        (key, note)
                    }
                })
                .collect();
        }
        _ => unreachable!("choice_criteria returns a list or a map"),
    }
    Ok(())
}

/// Sets option `index`'s description; empty text is `null`. A list of names becomes a map to hold it.
pub fn set_option_note(spec: &mut Value, index: usize, text: &str) -> Result<(), String> {
    let crit = choice_criteria(spec)?;
    if let Value::Array(items) = crit {
        if text.is_empty() {
            return Ok(());
        }
        let map = items
            .iter()
            .map(|name| (Field::of(Some(name)).text().to_string(), Value::Null))
            .collect();
        *crit = Value::Object(map);
    }
    let Value::Object(map) = crit else {
        unreachable!("choice criteria are a map here")
    };
    let (_, note) = map.iter_mut().nth(index).ok_or("no such option")?;
    *note = if text.is_empty() {
        Value::Null
    } else {
        json!(text)
    };
    Ok(())
}

pub fn add_option(spec: &mut Value) -> Result<(), String> {
    let crit = choice_criteria(spec)?;
    let names = option_names(crit);
    if names.len() >= *CHOICE_OPTIONS.end() {
        return Err(format!(
            "a choice has at most {} options",
            CHOICE_OPTIONS.end()
        ));
    }
    let name = unused_name(names.iter().map(String::as_str));
    match crit {
        Value::Array(items) => items.push(json!(name)),
        Value::Object(map) => {
            map.insert(name, Value::Null);
        }
        _ => unreachable!("choice_criteria returns a list or a map"),
    }
    Ok(())
}

pub fn remove_option(spec: &mut Value, index: usize) -> Result<(), String> {
    let crit = choice_criteria(spec)?;
    if option_names(crit).len() <= *CHOICE_OPTIONS.start() {
        return Err(format!(
            "a choice has at least {} options",
            CHOICE_OPTIONS.start()
        ));
    }
    match crit {
        Value::Array(items) if index < items.len() => {
            items.remove(index);
        }
        Value::Object(map) if index < map.len() => {
            let name = map.keys().nth(index).cloned().expect("the index exists");
            map.shift_remove(&name);
        }
        _ => return Err("no such option".into()),
    }
    Ok(())
}

fn option_names(crit: &Value) -> Vec<String> {
    match crit {
        Value::Array(items) => items
            .iter()
            .map(|name| Field::of(Some(name)).text().to_string())
            .collect(),
        Value::Object(map) => map.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `option_<n>` for the first `n`, from the option count plus one, that no option has.
fn unused_name<'a>(names: impl Iterator<Item = &'a str> + Clone) -> String {
    (names.clone().count() + 1..)
        .map(|n| format!("option_{n}"))
        .find(|candidate| !names.clone().any(|name| name == candidate))
        .expect("some option_<n> is free")
}

/// The score levels as a list, which a `{"0": ..}` legend becomes once the builder edits it.
fn score_levels(spec: &mut Value) -> Result<&mut Vec<Value>, String> {
    if !matches!(criteria(spec), Criteria::Score(_)) {
        return Err("not a score question with editable levels".into());
    }
    let crit = object(spec).entry("criteria").or_insert(Value::Null);
    if !crit.is_array() {
        let list = match crit {
            Value::Object(legend) => legend_levels(legend)
                .map(|levels| levels.into_iter().cloned().collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        *crit = Value::Array(list);
    }
    Ok(crit.as_array_mut().expect("just made a list"))
}

pub fn set_level(spec: &mut Value, index: usize, text: &str) -> Result<(), String> {
    let level = score_levels(spec)?.get_mut(index).ok_or("no such level")?;
    *level = json!(text);
    Ok(())
}

pub fn add_level(spec: &mut Value) -> Result<(), String> {
    let levels = score_levels(spec)?;
    if levels.len() >= *SCORE_LEVELS.end() {
        return Err(format!("a score has at most {} levels", SCORE_LEVELS.end()));
    }
    levels.push(json!(""));
    Ok(())
}

pub fn remove_level(spec: &mut Value, index: usize) -> Result<(), String> {
    let levels = score_levels(spec)?;
    if levels.len() <= *SCORE_LEVELS.start() {
        return Err(format!(
            "a score has at least {} levels",
            SCORE_LEVELS.start()
        ));
    }
    if index >= levels.len() {
        return Err("no such level".into());
    }
    levels.remove(index);
    Ok(())
}

/// Adds a noul question with blank instructions under the first free id `q<n>`; returns the id.
pub fn add_question(questions: &mut Questions) -> String {
    let id = (questions.len() + 1..)
        .map(|n| format!("q{n}"))
        .find(|id| !questions.contains_key(id))
        .expect("some q<n> is free");
    questions.insert(id.clone(), json!({"type": "noul", "instructions": ""}));
    id
}

/// Renames a question in place; the new id must be non-empty and unused.
pub fn rename_question(questions: &mut Questions, old: &str, new: &str) -> Result<(), String> {
    if old == new {
        return Ok(());
    }
    if new.is_empty() {
        return Err("a question id cannot be empty".into());
    }
    if questions.contains_key(new) {
        return Err(format!("another question is already named {new:?}"));
    }
    let index = questions.get_index_of(old).ok_or("no such question")?;
    let (_, spec) = questions
        .shift_remove_index(index)
        .expect("the index exists");
    questions.shift_insert(index, new.to_string(), spec);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noul_criteria_are_optional() {
        let mut q = json!({"type": "noul", "instructions": "Refund?"});
        set_noul(&mut q, true, "asks for money back").unwrap();
        set_noul(&mut q, false, "anything else").unwrap();
        assert_eq!(
            q["criteria"],
            json!({"true": "asks for money back", "false": "anything else"})
        );
        set_noul(&mut q, true, "").unwrap();
        assert_eq!(q["criteria"], json!({"false": "anything else"}));
        set_noul(&mut q, false, "").unwrap();
        assert_eq!(q, json!({"type": "noul", "instructions": "Refund?"}));
    }

    #[test]
    fn a_list_of_names_stays_a_list_until_a_description_is_added() {
        let mut q = json!({"type": "choice", "instructions": "Dept?", "criteria": ["billing", "technical"]});
        set_option_name(&mut q, 1, "tech").unwrap();
        add_option(&mut q).unwrap();
        assert_eq!(q["criteria"], json!(["billing", "tech", "option_3"]));
        assert!(
            set_option_name(&mut q, 2, "billing")
                .unwrap_err()
                .contains("already")
        );
        set_option_note(&mut q, 0, "").unwrap();
        assert!(q["criteria"].is_array());
        set_option_note(&mut q, 0, "money matters").unwrap();
        assert_eq!(
            serde_json::to_string(&q["criteria"]).unwrap(),
            r#"{"billing":"money matters","tech":null,"option_3":null}"#
        );
        set_option_name(&mut q, 0, "payments").unwrap();
        remove_option(&mut q, 1).unwrap();
        assert_eq!(
            serde_json::to_string(&q["criteria"]).unwrap(),
            r#"{"payments":"money matters","option_3":null}"#
        );
        assert!(remove_option(&mut q, 0).is_err());
        assert_eq!(
            criteria(&q),
            Criteria::Choice(vec![
                ("payments".into(), Field::Text("money matters".into())),
                ("option_3".into(), Field::Text(String::new())),
            ])
        );
    }

    #[test]
    fn choices_hold_2_to_255_options() {
        let names: Map<String, Value> = (0..254).map(|i| (format!("o{i}"), Value::Null)).collect();
        let mut q = json!({"type": "choice", "instructions": "?", "criteria": names});
        add_option(&mut q).unwrap();
        assert_eq!(q["criteria"].as_object().unwrap().len(), 255);
        assert!(add_option(&mut q).is_err());
    }

    #[test]
    fn scores_hold_2_to_10_levels() {
        let mut q = json!({"type": "score", "instructions": "Fit?", "criteria": {"1": "some", "0": "none"}});
        assert_eq!(
            criteria(&q),
            Criteria::Score(vec![Field::Text("none".into()), Field::Text("some".into())])
        );
        set_level(&mut q, 1, "partial").unwrap();
        assert_eq!(q["criteria"], json!(["none", "partial"]));
        for _ in 0..8 {
            add_level(&mut q).unwrap();
        }
        assert!(add_level(&mut q).is_err());
        for _ in 0..8 {
            remove_level(&mut q, 2).unwrap();
        }
        assert!(remove_level(&mut q, 0).is_err());
        assert_eq!(q["criteria"], json!(["none", "partial"]));
    }

    #[test]
    fn switching_types_carries_the_list() {
        let mut q = json!({"type": "choice", "instructions": "Tone?", "criteria": {"calm": "fine", "angry": null}});
        set_kind(&mut q, QuestionType::Score);
        assert_eq!(q["criteria"], json!(["calm", "angry"]));
        q["isolated"] = json!(false);
        set_kind(&mut q, QuestionType::Choice);
        assert_eq!(
            serde_json::to_string(&q["criteria"]).unwrap(),
            r#"{"calm":null,"angry":null}"#
        );
        assert!(q.get("isolated").is_none());
        set_kind(&mut q, QuestionType::Noul);
        assert_eq!(q, json!({"type": "noul", "instructions": "Tone?"}));
        set_kind(&mut q, QuestionType::Choice);
        assert_eq!(q["criteria"], json!({"option_1": null, "option_2": null}));
        set_kind(&mut q, QuestionType::Noul);
        set_kind(&mut q, QuestionType::Score);
        assert_eq!(q["criteria"], json!(["", ""]));
        let mut junk = json!("not an object");
        set_kind(&mut junk, QuestionType::Noul);
        assert_eq!(junk, json!({"type": "noul"}));
    }

    #[test]
    fn switching_back_restores_the_criteria() {
        let mut memory = KindMemory::new();
        let list = json!({"type": "choice", "instructions": "Dept?", "criteria": ["billing", "technical"]});
        let mut q = list.clone();
        switch_kind(&mut q, QuestionType::Score, &mut memory);
        assert_eq!(q["criteria"], json!(["billing", "technical"]));
        set_level(&mut q, 1, "tech").unwrap();
        switch_kind(&mut q, QuestionType::Noul, &mut memory);
        set_noul(&mut q, true, "yes").unwrap();
        switch_kind(&mut q, QuestionType::Choice, &mut memory);
        assert_eq!(q, list);
        switch_kind(&mut q, QuestionType::Score, &mut memory);
        assert_eq!(q["criteria"], json!(["billing", "tech"]));
        switch_kind(&mut q, QuestionType::Noul, &mut memory);
        assert_eq!(q["criteria"], json!({"true": "yes"}));
    }

    #[test]
    fn structure_is_left_to_the_json_editor() {
        let q = json!({"type": "noul", "instructions": {"ask": "refund?"}, "criteria": {"true": ["a"]}});
        assert_eq!(
            instructions(&q),
            Field::Structured(r#"{"ask":"refund?"}"#.into())
        );
        assert_eq!(
            criteria(&q),
            Criteria::Noul {
                yes: Field::Structured(r#"["a"]"#.into()),
                no: Field::Text(String::new())
            }
        );
        assert!(matches!(
            criteria(&json!({"type": "score", "criteria": "3 levels"})),
            Criteria::Unreadable(_)
        ));
        assert!(matches!(
            criteria(&json!({"type": "bool"})),
            Criteria::Unreadable(_)
        ));
    }

    #[test]
    fn questions_are_added_and_renamed_in_place() {
        let mut map = Questions::new();
        assert_eq!(add_question(&mut map), "q1");
        map.insert("q2".into(), json!({"type": "noul"}));
        assert_eq!(add_question(&mut map), "q3");
        rename_question(&mut map, "q1", "refund").unwrap();
        assert_eq!(map.keys().collect::<Vec<_>>(), ["refund", "q2", "q3"]);
        assert!(rename_question(&mut map, "q2", "q3").is_err());
        assert!(rename_question(&mut map, "q2", "").is_err());
        assert_eq!(map["refund"], json!({"type": "noul", "instructions": ""}));
    }
}
