//! [`Decider`]: a Jev request -> prompt rows -> label logits from a [`LoadedModel`] -> typed answers, as decider 1.6.0's
//! `/v1/systemone` (`decider/serve.py#systemone`) computes them, with one prompt row per decode; or, for a decision
//! model (a profile with a [`crate::profile::DecisionFormat`]), a Jev request -> one [`DecisionSequence`] per
//! question in THX-01's format ([`crate::thx01`]) -> one probability vector each from a [`DecisionModel`] -> the same
//! typed answers.

use ardana_api::{ErrorBody, SystemOneRequest, SystemOneResponse, Usage, ValidationItem};
use indexmap::IndexMap;
use serde_json::Value;
use tokenizers::Tokenizer;

use crate::profile::{AnswerType, ModelProfile};
use crate::prompt::{self, Item, LabelTable, PromptError, RowQuestion};
use crate::runtime::{LoadOptions, Loaded, LoadedModel};
use crate::systemone::{self, PlannedRow, RenderedQuestion, RowKind, RowSpan};
use crate::thx01;

/// Why a request cannot be answered.
#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    /// Bad input: `loc` names the offending part (`["body", "questions", <id>]`), `msg` is decider's message.
    #[error("{msg}")]
    Invalid { loc: Vec<String>, msg: String },
    /// The request is larger than the model or the limits allow; nothing was decoded.
    #[error("{0}")]
    Capacity(String),
    /// The tokenizer or the runtime failed.
    #[error("{0:#}")]
    Runtime(anyhow::Error),
}

impl From<PromptError> for DecideError {
    fn from(err: PromptError) -> Self {
        DecideError::Runtime(err.into())
    }
}

/// How the API answers a request it could not decide; the server and the playground's in-tab engine both answer with
/// these, so a refused request reads the same wherever it ran.
impl DecideError {
    /// 422 for bad input, 413 for too much input, 500 for a failing tokenizer or runtime.
    pub fn status(&self) -> u16 {
        match self {
            DecideError::Invalid { .. } => 422,
            DecideError::Capacity(_) => 413,
            DecideError::Runtime(_) => 500,
        }
    }

    /// One `value_error` at `loc` with decider's message, a `request_too_large` error, or an `api_error`.
    pub fn body(&self) -> ErrorBody {
        match self {
            DecideError::Invalid { loc, msg } => ErrorBody::validation(vec![ValidationItem::new(
                loc.iter().cloned().map(Value::String).collect(),
                msg.clone(),
                "value_error",
            )]),
            DecideError::Capacity(msg) => ErrorBody::error("request_too_large", msg.clone()),
            DecideError::Runtime(err) => ErrorBody::error("api_error", format!("{err:#}")),
        }
    }
}

/// Request size limits checked by [`Decider::plan`] before any decode. The defaults are decider's server limits and
/// the default [`LoadOptions`] window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Scoring rows per request (questions, with isolated score levels expanded).
    pub max_rows: usize,
    /// Tokens in one row.
    pub max_row_tokens: usize,
    /// Tokens over all rows of one request.
    pub max_request_tokens: usize,
    /// The model's context window; a row longer than this is refused instead of truncated. `None` checks it only
    /// in [`Decider::run`], against [`LoadedModel::n_ctx`]. A decision model's window is its profile's, whatever this
    /// says.
    pub context_window: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_rows: 1024,
            max_row_tokens: 36_864,
            max_request_tokens: 1 << 20,
            context_window: Some(LoadOptions::default().n_ctx as usize),
        }
    }
}

/// One question as a decision model reads it: its token ids in THX-01's format, the positions of its option markers
/// (one per option, in option order, each the mask token the model reads a logit at) and its answer type, which picks
/// the model's temperature bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionSequence {
    pub ids: Vec<u32>,
    pub markers: Vec<usize>,
    pub kind: AnswerType,
}

/// A validated, tokenized request, ready to run: prompt rows for a causal model, or sequences for a decision model.
#[derive(Debug, Clone)]
pub struct Plan {
    rqs: IndexMap<String, RenderedQuestion>,
    index: Vec<RowSpan>,
    rows: Vec<PlannedRow>,
    items: Vec<Item>,
    sequences: Vec<DecisionSequence>,
    input_tokens: usize,
}

impl Plan {
    /// Scoring rows (one decode each): the prompt rows, or the sequences.
    pub fn rows(&self) -> usize {
        self.items.len() + self.sequences.len()
    }

    /// Tokens of the longest row; 0 without rows.
    pub fn longest_row(&self) -> usize {
        self.row_ids().map(<[u32]>::len).max().unwrap_or(0)
    }

    /// Tokens over all rows.
    pub fn total_tokens(&self) -> usize {
        self.row_ids().map(<[u32]>::len).sum()
    }

    /// The `usage.input_tokens` the response reports: the shared state counts once in prompt rows; the sequences of a
    /// decision model count whole.
    pub fn input_tokens(&self) -> usize {
        self.input_tokens
    }

    /// The token ids of every row, in decode order.
    pub fn row_ids(&self) -> impl Iterator<Item = &[u32]> {
        self.items
            .iter()
            .map(|it| it.ids.as_slice())
            .chain(self.sequences.iter().map(|seq| seq.ids.as_slice()))
    }

    /// A decision model's sequences, one per question; empty for a causal model's plan.
    pub fn sequences(&self) -> &[DecisionSequence] {
        &self.sequences
    }
}

/// One row as [`Decider::run`] decodes it: [`LoadedModel::slot_logits`]'s arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decode<'a> {
    pub ids: &'a [u32],
    /// The answer slot of each question in the row (one per row in [`Decider::plan`]'s rows).
    pub slots: &'a [usize],
    /// The label ids every slot is read at, in label order.
    pub label_ids: &'a [u32],
}

/// Reads decider-format and stock models through one tokenizer and profile, and decision models through the same
/// tokenizer and the profile's [`crate::profile::DecisionFormat`].
#[derive(Debug, Clone)]
pub struct Decider {
    tokenizer: Tokenizer,
    profile: ModelProfile,
    labels: LabelTable,
    /// The special tokens of a decision model's sequences; `None` for a causal model.
    specials: Option<thx01::Specials>,
    limits: Limits,
}

impl Decider {
    pub fn new(tokenizer: Tokenizer, profile: ModelProfile) -> Result<Decider, DecideError> {
        let labels = LabelTable::new(&tokenizer)?;
        let specials = match profile.decision {
            Some(_) => Some(thx01::Specials::of(&tokenizer)?),
            None => None,
        };
        Ok(Decider {
            tokenizer,
            profile,
            labels,
            specials,
            limits: Limits::default(),
        })
    }

    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Decider {
        self.limits = limits;
        self
    }

    pub fn profile(&self) -> &ModelProfile {
        &self.profile
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    pub fn labels(&self) -> &LabelTable {
        &self.labels
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Validates every question (in request order, decider's messages), renders the state, plans one row per
    /// question (one per level for isolated score questions) and checks the limits. The state is never truncated.
    /// For a decision model: one sequence per question in THX-01's format, a choice question wider than the format
    /// allows refused as invalid (Q10) and a sequence longer than its window refused as too large (Q9).
    pub fn plan(&self, req: &SystemOneRequest) -> Result<Plan, DecideError> {
        let mut rqs = IndexMap::with_capacity(req.questions.len());
        for (id, spec) in &req.questions {
            let rq = systemone::render_question(spec).map_err(|msg| DecideError::Invalid {
                loc: vec!["body".into(), "questions".into(), id.clone()],
                msg,
            })?;
            rqs.insert(id.clone(), rq);
        }
        if let (Some(format), Some(specials)) = (&self.profile.decision, self.specials) {
            return self.plan_sequences(req, rqs, format, specials);
        }
        let (rows, index) = systemone::plan_rows(&rqs, self.profile.isolated_levels);
        let context = systemone::render_state(&req.state);
        let row_questions: Vec<Vec<RowQuestion<'_>>> = rows
            .iter()
            .map(|r| vec![(r.question.as_str(), r.options.as_slice())])
            .collect();
        let (items, ctx_len) = prompt::build_rows(
            &self.tokenizer,
            &self.labels,
            &self.profile.layout,
            &context,
            &row_questions,
            None,
        )?;
        let plan = Plan {
            input_tokens: prompt::unique_tokens(&items, ctx_len),
            rqs,
            index,
            rows,
            items,
            sequences: Vec::new(),
        };
        self.check(&plan, self.limits.context_window)?;
        Ok(plan)
    }

    /// The decision model's plan: every question one sequence over the serialised state, each read as one list of
    /// options (score levels are never isolated), `input_tokens` their lengths together.
    fn plan_sequences(
        &self,
        req: &SystemOneRequest,
        rqs: IndexMap<String, RenderedQuestion>,
        format: &crate::profile::DecisionFormat,
        specials: thx01::Specials,
    ) -> Result<Plan, DecideError> {
        let state = thx01::serialize_state(&req.state);
        let mut sequences = Vec::with_capacity(rqs.len());
        let mut index = Vec::with_capacity(rqs.len());
        for (first, (id, rq)) in rqs.iter().enumerate() {
            if rq.kind == AnswerType::Choice && rq.options.len() > format.max_options {
                return Err(DecideError::Invalid {
                    loc: vec!["body".into(), "questions".into(), id.clone()],
                    msg: format!(
                        "choice criteria: a map of 2..{} options (this model answers at most {} options in one pass)",
                        format.max_options, format.max_options
                    ),
                });
            }
            sequences.push(thx01::build_sequence(
                &self.tokenizer,
                specials,
                format,
                &state,
                rq,
            )?);
            index.push(RowSpan {
                id: id.clone(),
                kind: RowKind::List,
                first,
                count: 1,
            });
        }
        let plan = Plan {
            input_tokens: sequences.iter().map(|seq| seq.ids.len()).sum(),
            rqs,
            index,
            rows: Vec::new(),
            items: Vec::new(),
            sequences,
        };
        self.check(&plan, Some(format.window))?;
        Ok(plan)
    }

    /// decider's 413 checks in its order (rows, row tokens, request tokens), with the context window checked before
    /// the per-row limit.
    fn check(&self, plan: &Plan, context_window: Option<usize>) -> Result<(), DecideError> {
        let (n, longest, total) = (plan.rows(), plan.longest_row(), plan.total_tokens());
        let limits = &self.limits;
        let msg = if n > limits.max_rows {
            format!(
                "too many questions: the request expands to {n} scoring rows, the limit is {}",
                limits.max_rows
            )
        } else if let Some(window) = context_window.filter(|&w| longest > w) {
            format!(
                "the state and question need {longest} tokens, more than the model's context window of {window} tokens"
            )
        } else if longest > limits.max_row_tokens {
            format!(
                "too many tokens: one row has {longest} tokens, the limit is {} per row",
                limits.max_row_tokens
            )
        } else if total > limits.max_request_tokens {
            format!(
                "too many tokens: the request has {total} tokens over {n} rows, the limit is {}",
                limits.max_request_tokens
            )
        } else {
            return Ok(());
        };
        Err(DecideError::Capacity(msg))
    }

    /// What [`Decider::run`] asks the model for, row by row in decode order: the ids, the answer slots and the label
    /// ids each slot is read at. A runtime that cannot be called from `run` (an asynchronous one, in a browser)
    /// decodes these itself and hands the logits back through a [`LoadedModel`] that replays them. A decision model's
    /// plan has none.
    pub fn decodes<'a>(&'a self, plan: &'a Plan) -> impl Iterator<Item = Decode<'a>> {
        plan.items.iter().map(|item| {
            let width = item.nopts.iter().copied().max().unwrap_or(0);
            Decode {
                ids: &item.ids,
                slots: &item.slots,
                label_ids: &self.labels.ids[..width],
            }
        })
    }

    /// Decodes every row alone and reads the answers: per slot, softmax over the question's label logits divided by
    /// the temperature of its answer type.
    pub fn run(
        &self,
        model: &mut dyn LoadedModel,
        plan: &Plan,
    ) -> Result<SystemOneResponse, DecideError> {
        if !plan.sequences.is_empty() {
            return Err(DecideError::Runtime(anyhow::anyhow!(
                "the request was planned for a decision model, and a causal model cannot answer its sequences"
            )));
        }
        self.check(plan, Some(model.n_ctx()))?;
        let mut probs = Vec::with_capacity(plan.items.len());
        for ((decode, item), row) in self.decodes(plan).zip(&plan.items).zip(&plan.rows) {
            let logits = model
                .slot_logits(decode.ids, decode.slots, decode.label_ids)
                .map_err(DecideError::Runtime)?;
            let width = decode.label_ids.len();
            if logits.len() != item.slots.len() || logits.iter().any(|l| l.len() != width) {
                return Err(DecideError::Runtime(anyhow::anyhow!(
                    "the runtime returned {} logit rows for {} slots, expected {width} logits each",
                    logits.len(),
                    item.slots.len()
                )));
            }
            let temperature = self.profile.temperature_for(row.kind);
            for (slot_logits, &n) in logits.iter().zip(&item.nopts) {
                probs.push(softmax(&slot_logits[..n], temperature));
            }
        }
        Ok(self.response(plan, &probs))
    }

    /// Answers `plan` with a model of either kind: a causal model as [`Decider::run`] does, a decision model through
    /// [`crate::runtime::DecisionModel::decide`], one calibrated probability vector per sequence, assembled into the
    /// same typed answers. A plan made for the other kind is a runtime error.
    pub fn answer(
        &self,
        model: &mut Loaded,
        plan: &Plan,
    ) -> Result<SystemOneResponse, DecideError> {
        let model = match model {
            Loaded::Causal(model) => return self.run(model.as_mut(), plan),
            Loaded::Decision(model) => model,
        };
        if !plan.items.is_empty() {
            return Err(DecideError::Runtime(anyhow::anyhow!(
                "the request was planned for a causal model, and a decision model cannot answer its rows"
            )));
        }
        let probs = model
            .decide(&plan.sequences)
            .map_err(DecideError::Runtime)?;
        let widths: Vec<usize> = plan.sequences.iter().map(|seq| seq.markers.len()).collect();
        if probs.len() != widths.len()
            || probs
                .iter()
                .zip(&widths)
                .any(|(p, &width)| p.len() != width)
        {
            return Err(DecideError::Runtime(anyhow::anyhow!(
                "the runtime returned {} probability vectors of {:?} for {} sequences of {widths:?} options",
                probs.len(),
                probs.iter().map(Vec::len).collect::<Vec<_>>(),
                widths.len()
            )));
        }
        Ok(self.response(plan, &probs))
    }

    fn response(&self, plan: &Plan, probs: &[Vec<f64>]) -> SystemOneResponse {
        SystemOneResponse {
            model: self.profile.name.clone(),
            answers: systemone::assemble(&plan.rqs, &plan.index, probs),
            usage: Usage {
                input_tokens: plan.input_tokens as u64,
                output_tokens: 0,
            },
        }
    }

    /// [`Decider::plan`] then [`Decider::run`].
    pub fn decide(
        &self,
        model: &mut dyn LoadedModel,
        req: &SystemOneRequest,
    ) -> Result<SystemOneResponse, DecideError> {
        let plan = self.plan(req)?;
        self.run(model, &plan)
    }
}

/// `softmax(logits / T)` as decider's engine computes it on float32 logits: the division by the float32 temperature
/// and the max subtraction in f32, the exponentials and the normalisation in f64, each probability stored as f32.
pub fn softmax(logits: &[f32], temperature: f64) -> Vec<f64> {
    let t = temperature as f32;
    let z: Vec<f32> = logits.iter().map(|&x| x / t).collect();
    let max = z.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let e: Vec<f64> = z.iter().map(|&x| f64::from(x - max).exp()).collect();
    let sum: f64 = e.iter().sum();
    e.iter().map(|&x| f64::from((x / sum) as f32)).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::profile::{DecisionFormat, Layout};
    use crate::runtime::DecisionModel;
    use crate::thx01::{CLS_TOKEN, MASK_TOKEN, SEP_TOKEN, word_tokenizer};

    /// A decision-model reader over a word-level tokenizer that holds decider's 255 labels (`Decider::new` builds the
    /// label table whatever the model's kind) and a few words.
    fn decision_decider(window: usize) -> Decider {
        let letters: Vec<String> = ('A'..='Z').map(|c| c.to_string()).collect();
        let pairs: Vec<String> = ('A'..='Z')
            .flat_map(|a| ('A'..='Z').map(move |b| format!("{a}{b}")))
            .collect();
        let words: Vec<&str> = [
            "choice",
            "noul",
            "score",
            "question:",
            "Which",
            "team?",
            "Refund?",
            "How",
            "bad?",
            "billing",
            "sales",
            "tech",
            "the",
            "card",
            "level",
            "0:",
            "1:",
            "2:",
            "calm",
            "upset",
            "furious",
        ]
        .into_iter()
        .chain(letters.iter().map(String::as_str))
        .chain(pairs.iter().map(String::as_str))
        .collect();
        let tokenizer = word_tokenizer(&words, &[CLS_TOKEN, SEP_TOKEN, MASK_TOKEN]);
        let profile = ModelProfile {
            decision: Some(DecisionFormat {
                window,
                max_options: 24,
                head_max_len: 32,
            }),
            ..ModelProfile::stock("THX-test", Layout::Plain)
        };
        Decider::new(tokenizer, profile).unwrap()
    }

    fn request(state: &str, questions: Value) -> SystemOneRequest {
        serde_json::from_value(json!({"state": state, "questions": questions})).unwrap()
    }

    fn three_questions() -> Value {
        json!({
            "team": {"type": "choice", "instructions": "Which team?", "criteria": ["billing", "sales", "tech"]},
            "refund": {"type": "noul", "instructions": "Refund?"},
            "mood": {"type": "score", "instructions": "How bad?", "criteria": ["calm", "upset", "furious"]},
        })
    }

    /// A decision model answering every sequence with the next of its vectors.
    struct Probs(Vec<Vec<f64>>);

    impl DecisionModel for Probs {
        fn decide(&mut self, _: &[DecisionSequence]) -> anyhow::Result<Vec<Vec<f64>>> {
            Ok(self.0.clone())
        }
    }

    /// A causal model answering every slot with equal logits.
    struct Flat;

    impl LoadedModel for Flat {
        fn n_ctx(&self) -> usize {
            1024
        }

        fn slot_logits(
            &mut self,
            _: &[u32],
            slots: &[usize],
            label_ids: &[u32],
        ) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(vec![vec![0.0; label_ids.len()]; slots.len()])
        }
    }

    /// A profile with a `decision` plans one sequence per question, never isolated, each of its kind with one marker
    /// per option, `input_tokens` their lengths together and no prompt rows to decode; `answer` assembles a decision
    /// model's vectors into the typed answers, and a model of the other kind, or a model answering the wrong number
    /// of options, is a runtime error.
    #[test]
    fn decision_plans_answer_through_decide() {
        let d = decision_decider(1024);
        let plan = d.plan(&request("the card", three_questions())).unwrap();
        let sequences = plan.sequences();
        assert_eq!((plan.rows(), sequences.len()), (3, 3));
        assert_eq!(
            sequences.iter().map(|s| s.kind).collect::<Vec<_>>(),
            [AnswerType::Choice, AnswerType::Noul, AnswerType::Score]
        );
        assert_eq!(
            sequences
                .iter()
                .map(|s| s.markers.len())
                .collect::<Vec<_>>(),
            [3, 2, 3]
        );
        assert_eq!(
            plan.input_tokens(),
            sequences.iter().map(|s| s.ids.len()).sum::<usize>()
        );
        assert_eq!(plan.total_tokens(), plan.input_tokens());
        assert_eq!(
            plan.longest_row(),
            sequences.iter().map(|s| s.ids.len()).max().unwrap()
        );
        assert_eq!(d.decodes(&plan).count(), 0);
        assert_eq!(plan.row_ids().count(), 3);

        let probs = vec![vec![0.2, 0.7, 0.1], vec![0.25, 0.75], vec![0.1, 0.3, 0.6]];
        let mut model = Loaded::Decision(Box::new(Probs(probs)));
        let response = d.answer(&mut model, &plan).unwrap();
        assert_eq!(response.model, "THX-test");
        assert_eq!(response.usage.input_tokens as usize, plan.input_tokens());
        let answers = serde_json::to_value(&response.answers).unwrap();
        assert_eq!(answers["team"]["choice"], "sales");
        assert_eq!(answers["team"]["probabilities"]["billing"], 0.2);
        assert_eq!(answers["refund"]["noul"], 0.75);
        assert_eq!(answers["mood"]["score"], 1.5);
        assert_eq!(answers["mood"]["probabilities"]["2"], 0.6);
        assert!(
            answers["mood"]
                .get("x_level_fit")
                .is_none_or(Value::is_null)
        );

        let mut short = Loaded::Decision(Box::new(Probs(vec![vec![0.5, 0.5]; 3])));
        let err = d.answer(&mut short, &plan).unwrap_err();
        assert!(matches!(err, DecideError::Runtime(_)), "{err}");
        assert!(err.to_string().contains("probability vectors"), "{err}");
        let err = d.run(&mut Flat, &plan).unwrap_err();
        assert!(err.to_string().contains("decision model"), "{err}");
        let mut causal = Loaded::Causal(Box::new(Flat));
        let err = d.answer(&mut causal, &plan).unwrap_err();
        assert!(err.to_string().contains("decision model"), "{err}");

        // A causal plan is no decision model's.
        let stock = Decider::new(
            d.tokenizer().clone(),
            ModelProfile::stock("stock", Layout::Plain),
        )
        .unwrap();
        let causal_plan = stock.plan(&request("the card", three_questions())).unwrap();
        assert!(causal_plan.sequences().is_empty());
        assert_eq!(causal_plan.rows(), 3);
        let err = stock.answer(&mut model, &causal_plan).unwrap_err();
        assert!(err.to_string().contains("causal model"), "{err}");
        assert!(stock.answer(&mut causal, &causal_plan).is_ok());
    }

    /// Q9, Q10: a choice question above the format's option limit is invalid at its own location, naming the limit;
    /// a sequence longer than the format's window is too large, naming the window, whatever `Limits::context_window`
    /// says, and one inside it plans.
    #[test]
    fn decision_plans_refuse_wide_choices_and_long_sequences() {
        let d = decision_decider(1024);
        let wide: Vec<String> = (0..25).map(|i| format!("o{i}")).collect();
        let err = d
            .plan(&request(
                "the card",
                json!({"many": {"type": "choice", "instructions": "Which team?", "criteria": wide}}),
            ))
            .unwrap_err();
        match &err {
            DecideError::Invalid { loc, msg } => {
                assert_eq!(loc, &["body", "questions", "many"]);
                assert!(msg.contains("24"), "{msg}");
            }
            other => panic!("{other}"),
        }
        assert_eq!(err.status(), 422);
        let narrow: Vec<String> = wide[..24].to_vec();
        assert!(
            d.plan(&request(
                "the card",
                json!({"many": {"type": "choice", "instructions": "Which team?", "criteria": narrow}}),
            ))
            .is_ok()
        );

        let d = decision_decider(40).with_limits(Limits {
            context_window: Some(5),
            ..Limits::default()
        });
        let long = vec!["card"; 40].join(" ");
        let err = d.plan(&request(&long, three_questions())).unwrap_err();
        match &err {
            DecideError::Capacity(msg) => {
                assert!(msg.contains("context window of 40 tokens"), "{msg}");
            }
            other => panic!("{other}"),
        }
        assert_eq!(err.status(), 413);
        let plan = d.plan(&request("the card", three_questions())).unwrap();
        assert!(plan.longest_row() > 5 && plan.longest_row() <= 40);
    }
}
