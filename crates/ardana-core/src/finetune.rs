//! What `ardana finetune` trains on: labelled lines read into [`Example`]s, split by line into a train, a calibration
//! and a test split, and the train and calibration lines turned into [`TrainingRow`]s, which are the rows
//! [`Decider::plan`] builds for the same request with the gold option's label id appended, so the trainer and every
//! later run read the same tokens.
//!
//! A line is `{"state": <string | JSON>, "questions"?: {<id>: <question spec>}, "answers": {<id>: <label>}}`: its own
//! questions, else the ones of the `--questions` file; a label is the option's name (choice), `true` or `false`
//! (noul), the level's index for list criteria and its key for a legend (score), and `null` leaves the question
//! unlabelled.

use ardana_api::SystemOneRequest;
use indexmap::IndexMap;
use serde_json::Value;

use crate::engine::{DecideError, Decider};
use crate::py;
use crate::systemone::{Names, RenderedQuestion, render_question};

/// Fewer labelled lines than this are refused.
pub const MIN_LINES: usize = 100;
/// An option with fewer training examples than this is warned about.
pub const MIN_OPTION_EXAMPLES: usize = 50;
/// The seed of the split.
pub const SEED: u64 = 0;

/// One labelled line: its request, without `model`, and the labels of the questions it labels, in request order.
#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    /// The line's number in its file, from 1.
    pub line: usize,
    pub request: SystemOneRequest,
    pub labels: IndexMap<String, Label>,
}

/// The gold answer of one question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// The option's name.
    Choice(String),
    Noul(bool),
    /// The level's name: its index (`"0"`, `"1"`, ...) for list criteria, its key for a legend.
    Score(String),
}

impl Label {
    /// The label `value` gives the question `rq`; `None` for `null`. The message says what the question takes.
    fn read(value: &Value, rq: &RenderedQuestion) -> Result<Option<Label>, String> {
        if value.is_null() {
            return Ok(None);
        }
        let label = match (&rq.names, value) {
            (Names::Choice(_), Value::String(name)) => Label::Choice(name.clone()),
            (Names::Choice(names), other) => {
                return Err(format!(
                    "a choice label is the name of one of its options ({}), got {}",
                    names.join(", "),
                    py::repr(other)
                ));
            }
            (Names::Noul, Value::Bool(yes)) => Label::Noul(*yes),
            (Names::Noul, other) => {
                return Err(format!(
                    "a noul label is true or false, got {}",
                    py::repr(other)
                ));
            }
            (Names::Score(_), Value::String(name)) => Label::Score(name.clone()),
            (Names::Score(_), Value::Number(n)) if n.as_u64().is_some() => {
                Label::Score(n.to_string())
            }
            (Names::Score(names), other) => {
                return Err(format!(
                    "a score label is a level's index for list criteria and its key for a legend ({}), got {}",
                    names.join(", "),
                    py::repr(other)
                ));
            }
        };
        if label.option(rq).is_none() {
            return Err(format!(
                "{} is not one of its options ({})",
                py::repr(value),
                option_names(rq).join(", ")
            ));
        }
        Ok(Some(label))
    }

    /// The position of the option this label names among the options of `rq`, in label order (a noul's `no` first).
    pub fn option(&self, rq: &RenderedQuestion) -> Option<usize> {
        match (self, &rq.names) {
            (Label::Choice(name), Names::Choice(names))
            | (Label::Score(name), Names::Score(names)) => names.iter().position(|n| n == name),
            (Label::Noul(yes), Names::Noul) => Some(usize::from(*yes)),
            _ => None,
        }
    }

    /// The option's name as a line writes it (`true` and `false` for a noul).
    pub fn name(&self) -> String {
        match self {
            Label::Choice(name) | Label::Score(name) => name.clone(),
            Label::Noul(yes) => yes.to_string(),
        }
    }
}

/// The names a label gives the options of `rq`, in label order.
fn option_names(rq: &RenderedQuestion) -> Vec<String> {
    match &rq.names {
        Names::Choice(names) | Names::Score(names) => names.clone(),
        Names::Noul => vec!["false".into(), "true".into()],
    }
}

/// One training sequence: a planned row's ids with the gold option's label id appended, and the mask of the tokens
/// the loss reads, that label alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingRow {
    pub input_ids: Vec<u32>,
    pub completion_mask: Vec<u8>,
}

/// The rows of `example` that carry a label, in decode order: each planned row's ids ([`Decider::decodes`]) and the
/// label id of the gold option at its answer slot; a row of an isolated score level takes `yes` for the gold level
/// and `no` for every other. A decision model's plan has no rows.
pub fn training_rows(
    decider: &Decider,
    example: &Example,
) -> Result<Vec<TrainingRow>, DecideError> {
    let plan = decider.plan(&example.request)?;
    let mut rows = Vec::new();
    for (decode, (id, level)) in decider.decodes(&plan).zip(plan.row_questions()) {
        let Some(label) = example.labels.get(id) else {
            continue;
        };
        let rq = plan
            .question(id)
            .expect("a planned row's question is in its plan");
        let gold = label.option(rq).ok_or_else(|| DecideError::Invalid {
            loc: vec!["answers".into(), id.to_string()],
            msg: format!("{} is not one of its options", label.name()),
        })?;
        let option = level.map_or(gold, |level| usize::from(level == gold));
        if decode.slots != [decode.ids.len() - 1] {
            return Err(DecideError::Runtime(anyhow::anyhow!(
                "the row of question {id} reads its answer at {:?}, not at its last token",
                decode.slots
            )));
        }
        let mut input_ids = decode.ids.to_vec();
        input_ids.push(decode.label_ids[option]);
        let mut completion_mask = vec![0; decode.ids.len()];
        completion_mask.push(1);
        rows.push(TrainingRow {
            input_ids,
            completion_mask,
        });
    }
    Ok(rows)
}

/// Why the data cannot be trained on, naming the file, the line and the question.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DataError {
    #[error("{file}:{line}: {msg}")]
    Line {
        file: String,
        line: usize,
        msg: String,
    },
    #[error("{file}:{line}: question {question}: {msg}")]
    Question {
        file: String,
        line: usize,
        question: String,
        msg: String,
    },
    #[error("{file}: {msg}")]
    File { file: String, msg: String },
}

/// A data file as read: its labelled lines and how many lines labelled nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Data {
    pub examples: Vec<Example>,
    pub unlabelled: usize,
}

/// The questions of a `--questions` file named `file`: a map of question id to question spec, or a `/v1/systemone`
/// body (an object with a `state` and a `questions` map), whose questions it gives.
pub fn read_questions(text: &str, file: &str) -> Result<IndexMap<String, Value>, DataError> {
    let invalid = |msg: String| DataError::File {
        file: file.to_string(),
        msg,
    };
    let value: Value =
        serde_json::from_str(text).map_err(|err| invalid(format!("invalid JSON: {err}")))?;
    let Value::Object(map) = value else {
        return Err(invalid(
            "a questions file is a map of question id to question spec, or a /v1/systemone body"
                .into(),
        ));
    };
    let questions = match (map.get("state"), map.get("questions")) {
        (Some(_), Some(Value::Object(questions))) => questions.clone(),
        _ => map,
    };
    if questions.is_empty() {
        return Err(invalid("it asks no question".into()));
    }
    Ok(questions.into_iter().collect())
}

/// Reads the JSONL `text` of the file `file`: every line that labels a question becomes an [`Example`], each question
/// validated by [`Decider::plan`] and each label checked against its question; a line whose answers are all `null`
/// is counted and skipped, a blank line skipped. A line without questions of its own asks `questions`. Fewer than
/// [`MIN_LINES`] labelled lines are refused.
pub fn read_jsonl(
    text: &str,
    file: &str,
    questions: Option<&IndexMap<String, Value>>,
    decider: &Decider,
) -> Result<Data, DataError> {
    let mut data = Data {
        examples: Vec::new(),
        unlabelled: 0,
    };
    for (i, text) in text.lines().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        match read_line(text, i + 1, file, questions, decider)? {
            Some(example) => data.examples.push(example),
            None => data.unlabelled += 1,
        }
    }
    if data.examples.len() < MIN_LINES {
        return Err(DataError::File {
            file: file.to_string(),
            msg: format!(
                "{} labelled lines; fine-tuning needs at least {MIN_LINES}",
                data.examples.len()
            ),
        });
    }
    Ok(data)
}

/// One line of [`read_jsonl`]: its example, or `None` when it labels nothing.
fn read_line(
    text: &str,
    line: usize,
    file: &str,
    shared: Option<&IndexMap<String, Value>>,
    decider: &Decider,
) -> Result<Option<Example>, DataError> {
    let at_line = |msg: String| DataError::Line {
        file: file.to_string(),
        line,
        msg,
    };
    let at_question = |question: &str, msg: String| DataError::Question {
        file: file.to_string(),
        line,
        question: question.to_string(),
        msg,
    };
    let value: Value =
        serde_json::from_str(text).map_err(|err| at_line(format!("invalid JSON: {err}")))?;
    let Value::Object(mut fields) = value else {
        return Err(at_line(format!(
            "a line is an object with a state and answers, got {}",
            py::repr(&value)
        )));
    };
    let state = fields
        .remove("state")
        .ok_or_else(|| at_line("the line has no state".into()))?;
    let questions: IndexMap<String, Value> = match fields.remove("questions") {
        Some(Value::Object(own)) => own.into_iter().collect(),
        Some(other) => {
            return Err(at_line(format!(
                "questions is a map of question id to question spec, got {}",
                py::repr(&other)
            )));
        }
        None => shared.cloned().ok_or_else(|| {
            at_line("the line asks no questions, and no --questions file names any".into())
        })?,
    };
    let answers = match fields.remove("answers") {
        Some(Value::Object(answers)) => answers,
        Some(other) => {
            return Err(at_line(format!(
                "answers is a map of question id to label, got {}",
                py::repr(&other)
            )));
        }
        None => return Err(at_line("the line has no answers".into())),
    };
    if let Some(id) = answers.keys().find(|id| !questions.contains_key(*id)) {
        return Err(at_question(
            id,
            "the line answers a question it does not ask".into(),
        ));
    }
    if answers.values().all(Value::is_null) {
        return Ok(None);
    }
    let request = SystemOneRequest {
        model: None,
        state,
        questions,
    };
    let plan = decider.plan(&request).map_err(|err| match err {
        DecideError::Invalid { loc, msg } => match loc.last() {
            Some(id) if request.questions.contains_key(id) => at_question(id, msg),
            _ => at_line(msg),
        },
        DecideError::Capacity(msg) => at_line(format!("too large: {msg}")),
        DecideError::Runtime(err) => at_line(format!("{err:#}")),
    })?;
    let mut labels = IndexMap::new();
    for (id, value) in &answers {
        let rq = plan
            .question(id)
            .expect("every answered question was planned");
        if let Some(label) = Label::read(value, rq).map_err(|msg| at_question(id, msg))? {
            labels.insert(id.clone(), label);
        }
    }
    Ok(Some(Example {
        line,
        request,
        labels,
    }))
}

/// The examples of each split, in line order.
#[derive(Debug, Clone, PartialEq)]
pub struct Split<'a> {
    pub train: Vec<&'a Example>,
    pub calibration: Vec<&'a Example>,
    pub test: Vec<&'a Example>,
}

/// Splits `examples` by line: shuffled with [`SEED`], a tenth (rounded down) to calibration, a tenth to test and the
/// rest, about 80%, to train; the same examples give the same split every time.
pub fn split(examples: &[Example]) -> Split<'_> {
    let mut order: Vec<usize> = (0..examples.len()).collect();
    let mut state = SEED;
    for i in (1..order.len()).rev() {
        let j = (splitmix64(&mut state) % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    let tenth = examples.len() / 10;
    let pick = |positions: &[usize]| {
        let mut picked: Vec<&Example> = positions.iter().map(|&i| &examples[i]).collect();
        picked.sort_by_key(|example| example.line);
        picked
    };
    Split {
        calibration: pick(&order[..tenth]),
        test: pick(&order[tenth..2 * tenth]),
        train: pick(&order[2 * tenth..]),
    }
}

/// SplitMix64's next value of `state`.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// An option the training split labels fewer than [`MIN_OPTION_EXAMPLES`] times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sparse {
    pub question: String,
    /// The option's name as a label writes it.
    pub option: String,
    pub examples: usize,
}

/// Every option of a labelled question (a noul's `true` and `false` sides included) that fewer than
/// [`MIN_OPTION_EXAMPLES`] of `train` label, in the order the questions and their options come.
pub fn sparse_options(train: &[&Example]) -> Vec<Sparse> {
    let mut counts: IndexMap<(String, String), usize> = IndexMap::new();
    for example in train {
        for (id, label) in &example.labels {
            if !counts.keys().any(|(question, _)| question == id)
                && let Ok(rq) = render_question(&example.request.questions[id])
            {
                for option in option_names(&rq) {
                    counts.insert((id.clone(), option), 0);
                }
            }
            *counts.entry((id.clone(), label.name())).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, n)| *n < MIN_OPTION_EXAMPLES)
        .map(|((question, option), examples)| Sparse {
            question,
            option,
            examples,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::chat::{chat_layout, read_template};
    use crate::profile::{Layout, ModelProfile, from_decider_config};
    use crate::prompt::encode;

    /// The pinned snapshot directory of `repo` in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
    fn snapshot(repo: &str) -> PathBuf {
        let dir = PathBuf::from(std::env::var_os("HF_HOME").expect("cargo sets HF_HOME"))
            .join("hub")
            .join(format!("models--{}", repo.replace('/', "--")));
        let commit = std::fs::read_to_string(dir.join("refs/main"))
            .unwrap_or_else(|err| panic!("{}: {err}; run `cargo xtask fetch`", dir.display()));
        dir.join("snapshots").join(commit.trim())
    }

    fn tokenizer(dir: &std::path::Path) -> tokenizers::Tokenizer {
        let path = dir.join("tokenizer.json");
        tokenizers::Tokenizer::from_file(&path)
            .unwrap_or_else(|err| panic!("{}: {err}; run `cargo xtask fetch`", path.display()))
    }

    /// decider:2b in its own profile: the plain layout, score levels isolated.
    fn decider_2b() -> Decider {
        let dir = snapshot("Mapika/decider-2b-GGUF");
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("decider_config.json")).unwrap(),
        )
        .unwrap();
        let profile = from_decider_config(&config, Layout::Plain).unwrap();
        assert!(profile.isolated_levels);
        Decider::new(tokenizer(&dir), profile).unwrap()
    }

    /// Qwen3 read zero-shot: the chat layout of its own template, score levels listwise.
    fn qwen3() -> Decider {
        let dir = snapshot("Qwen/Qwen3-1.7B");
        let tok = tokenizer(&dir);
        let (template, specials) = read_template(&dir).unwrap();
        let layout = chat_layout(&template, &tok, &specials).unwrap();
        Decider::new(tok, ModelProfile::stock("qwen3:1.7b", layout)).unwrap()
    }

    fn questions() -> IndexMap<String, Value> {
        serde_json::from_value(json!({
            "team": {"type": "choice", "instructions": "Which team should handle it?", "criteria": ["billing", "sales", "tech"]},
            "refund": {"type": "noul", "instructions": "Does the customer ask for a refund?"},
            "mood": {"type": "score", "instructions": "How upset is the customer?", "criteria": ["calm", "annoyed", "furious"]},
            "urgency": {"type": "score", "instructions": "How urgent is it?", "criteria": {"1": "whenever", "5": "now", "3": "today"}},
            "order": {"type": "noul", "instructions": "Is an order number given?"},
        }))
        .unwrap()
    }

    fn example(line: usize, labels: &[(&str, Label)]) -> Example {
        Example {
            line,
            request: SystemOneRequest {
                model: None,
                state: json!({"ticket": "I was charged twice for order A-104 and nobody answers.", "line": line}),
                questions: questions(),
            },
            labels: labels
                .iter()
                .map(|(id, label)| (id.to_string(), label.clone()))
                .collect(),
        }
    }

    /// R1.3: every training row is an inference row: its ids without the last are the ids `Decider::decodes` yields
    /// for the same request, its last is the gold option's label id (`yes` for the gold level of an isolated score
    /// question, `no` for every other level), and its mask marks that token alone; an unlabelled question has no
    /// rows. For decider's plain profile with isolated score levels and for Qwen3's chat template.
    #[test]
    fn training_rows_are_inference_rows() {
        let labels = [
            ("team", Label::Choice("tech".into())),
            ("refund", Label::Noul(true)),
            ("mood", Label::Score("2".into())),
            ("urgency", Label::Score("3".into())),
        ];
        for (decider, isolated) in [(decider_2b(), true), (qwen3(), false)] {
            let example = example(7, &labels);
            let rows = training_rows(&decider, &example).unwrap();
            let plan = decider.plan(&example.request).unwrap();
            let tok = decider.tokenizer();
            let id = |text: &str| match encode(tok, text).unwrap()[..] {
                [id] => id,
                ref ids => panic!("{text:?} is {ids:?}"),
            };
            let (no, yes) = (id("A"), id("B"));
            // Isolated levels are three rows a score question, each a no/yes question; listwise, one row of three.
            let wanted: Vec<(&str, u32)> = if isolated {
                vec![
                    ("team", id("C")),
                    ("refund", yes),
                    ("mood", no),
                    ("mood", no),
                    ("mood", yes),
                    ("urgency", no),
                    ("urgency", yes),
                    ("urgency", no),
                ]
            } else {
                vec![
                    ("team", id("C")),
                    ("refund", yes),
                    ("mood", id("C")),
                    ("urgency", id("B")),
                ]
            };
            let decodes: Vec<_> = decider
                .decodes(&plan)
                .zip(plan.row_questions())
                .filter(|(_, (question, _))| *question != "order")
                .collect();
            assert_eq!(rows.len(), wanted.len());
            assert_eq!(decodes.len(), wanted.len());
            for ((row, (decode, (question, _))), (want_question, label)) in
                rows.iter().zip(&decodes).zip(&wanted)
            {
                assert_eq!(question, want_question);
                let n = row.input_ids.len();
                assert_eq!(&row.input_ids[..n - 1], decode.ids, "{question}");
                assert_eq!(row.input_ids[n - 1], *label, "{question}");
                assert_eq!(decode.slots, [n - 2]);
                assert_eq!(decode.label_ids[..2], [no, yes]);
                let mut mask = vec![0; n - 1];
                mask.push(1);
                assert_eq!(row.completion_mask, mask, "{question}");
            }
        }
    }

    fn lines(lines: &[Value]) -> String {
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `n` valid lines of the shared questions, each labelling the team as `team(i)` and the refund as `i` is even.
    fn valid(n: usize, team: impl Fn(usize) -> &'static str) -> Vec<Value> {
        (0..n)
            .map(|i| {
                json!({"state": format!("ticket {i}"), "answers": {"team": team(i), "refund": i % 2 == 0, "mood": null}})
            })
            .collect()
    }

    /// R1.4: invalid JSON, a question spec `Decider::plan` refuses, an answer for a question the line does not ask, a
    /// label the question has no option for and a label of the wrong type each stop the read at their line, naming
    /// the file, the line and the question; a line labelling nothing is counted and skipped; fewer than 100
    /// labelled lines are refused; an option under 50 training examples is reported. A line's own questions win
    /// over the shared ones, which a questions file gives as a map or as a `/v1/systemone` body.
    #[test]
    fn data_errors_name_line_and_question() {
        let decider = decider_2b();
        let shared = questions();
        let read = |text: &str| read_jsonl(text, "data.jsonl", Some(&shared), &decider);
        let err = |bad: Value| {
            let mut all = valid(120, |_| "billing");
            all.insert(2, bad);
            read(&lines(&all)).unwrap_err().to_string()
        };
        let mut broken = lines(&valid(120, |_| "billing"));
        broken.insert_str(0, "{\"state\": \n");
        assert!(
            read(&broken)
                .unwrap_err()
                .to_string()
                .starts_with("data.jsonl:1: invalid JSON"),
        );
        assert_eq!(
            err(
                json!({"state": "s", "questions": {"pick": {"type": "choice", "instructions": "Which?", "criteria": ["only"]}}, "answers": {"pick": "only"}})
            ),
            "data.jsonl:3: question pick: choice criteria: a map of 2..255 options"
        );
        assert_eq!(
            err(json!({"state": "s", "answers": {"teams": "billing"}})),
            "data.jsonl:3: question teams: the line answers a question it does not ask"
        );
        assert_eq!(
            err(json!({"state": "s", "answers": {"team": "legal"}})),
            "data.jsonl:3: question team: 'legal' is not one of its options (billing, sales, tech)"
        );
        assert_eq!(
            err(json!({"state": "s", "answers": {"urgency": 2}})),
            "data.jsonl:3: question urgency: 2 is not one of its options (1, 3, 5)"
        );
        assert_eq!(
            err(json!({"state": "s", "answers": {"refund": "yes"}})),
            "data.jsonl:3: question refund: a noul label is true or false, got 'yes'"
        );
        assert_eq!(
            err(json!({"state": "s", "answers": {"team": 1}})),
            "data.jsonl:3: question team: a choice label is the name of one of its options (billing, sales, tech), got 1"
        );
        assert_eq!(
            err(json!({"state": "s", "answers": {"mood": 1.5}})),
            "data.jsonl:3: question mood: a score label is a level's index for list criteria and its key for a legend (0, 1, 2), got 1.5"
        );
        assert_eq!(
            err(json!({"answers": {"team": "sales"}})),
            "data.jsonl:3: the line has no state"
        );

        // Lines labelling nothing are counted, blank lines ignored, and labels read by type: a list level by its
        // index, a legend's level by its key.
        let mut all = valid(100, |i| ["billing", "sales", "tech"][i % 3]);
        all.push(json!({"state": "s", "answers": {"team": null, "refund": null}}));
        all.push(json!({"state": "s", "answers": {}}));
        all.push(json!({"state": "s", "answers": {"mood": 2, "urgency": "5"}}));
        all.push(
            json!({"state": "s", "questions": {"own": {"type": "noul", "instructions": "Own?"}}, "answers": {"own": false}}),
        );
        let data = read(&format!("{}\n\n", lines(&all))).unwrap();
        assert_eq!((data.examples.len(), data.unlabelled), (102, 2));
        let scored = &data.examples[100];
        assert_eq!(scored.line, 103);
        assert_eq!(
            scored.labels.values().collect::<Vec<_>>(),
            [&Label::Score("2".into()), &Label::Score("5".into())]
        );
        let own = &data.examples[101];
        assert_eq!(own.request.questions.keys().collect::<Vec<_>>(), ["own"]);
        assert_eq!(own.labels["own"], Label::Noul(false));

        // A line without questions of its own needs the shared ones.
        let err = read_jsonl(
            &lines(&valid(100, |_| "sales")),
            "data.jsonl",
            None,
            &decider,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "data.jsonl:1: the line asks no questions, and no --questions file names any"
        );
        let err = read(&lines(&valid(99, |_| "sales"))).unwrap_err();
        assert_eq!(
            err.to_string(),
            "data.jsonl: 99 labelled lines; fine-tuning needs at least 100"
        );

        // Options under 50 training examples: sales and tech here, and no option of an unlabelled question.
        let data = read(&lines(&valid(150, |i| match i % 10 {
            0 => "sales",
            _ => "billing",
        })))
        .unwrap();
        let train: Vec<&Example> = data.examples.iter().collect();
        let sparse: Vec<(String, String, usize)> = sparse_options(&train)
            .into_iter()
            .map(|s| (s.question, s.option, s.examples))
            .collect();
        assert_eq!(
            sparse,
            [
                ("team".to_string(), "sales".to_string(), 15),
                ("team".to_string(), "tech".to_string(), 0),
            ]
        );

        // A questions file is a map, or a body whose questions it gives.
        let map = json!({"refund": {"type": "noul", "instructions": "Refund?"}});
        let body = json!({"state": "s", "questions": map});
        for text in [map.to_string(), body.to_string()] {
            let read = read_questions(&text, "q.json").unwrap();
            assert_eq!(read.keys().collect::<Vec<_>>(), ["refund"]);
        }
        assert_eq!(
            read_questions("[]", "q.json").unwrap_err().to_string(),
            "q.json: a questions file is a map of question id to question spec, or a /v1/systemone body"
        );
    }

    /// R1.5: the same examples give the same 80/10/10 split every time, by line, every line in exactly one split.
    #[test]
    fn split_is_stable_and_disjoint() {
        let examples: Vec<Example> = (1..=1000)
            .map(|line| example(line, &[("refund", Label::Noul(line % 3 == 0))]))
            .collect();
        let first = split(&examples);
        assert_eq!(first, split(&examples));
        assert_eq!(
            (first.train.len(), first.calibration.len(), first.test.len()),
            (800, 100, 100)
        );
        let mut seen: Vec<usize> = first
            .train
            .iter()
            .chain(&first.calibration)
            .chain(&first.test)
            .map(|example| example.line)
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (1..=1000).collect::<Vec<_>>());
        // Shuffled, not cut in file order: each split takes lines from all over the file.
        for part in [&first.train, &first.calibration, &first.test] {
            assert!(part.windows(2).all(|w| w[0].line < w[1].line));
            assert!(part.first().unwrap().line < 100 && part.last().unwrap().line > 900);
        }
        let odd = split(&examples[..105]);
        assert_eq!(
            (odd.train.len(), odd.calibration.len(), odd.test.len()),
            (85, 10, 10)
        );
    }
}
