//! [`Decider`]: a Jev request -> prompt rows -> label logits from a [`LoadedModel`] -> typed answers, as decider 1.6.0's
//! `/v1/systemone` (`decider/serve.py#systemone`) computes them, with one prompt row per decode.

use ardana_api::{SystemOneRequest, SystemOneResponse, Usage};
use indexmap::IndexMap;
use tokenizers::Tokenizer;

use crate::profile::ModelProfile;
use crate::prompt::{self, Item, LabelTable, PromptError, RowQuestion};
use crate::runtime::{LoadOptions, LoadedModel};
use crate::systemone::{self, PlannedRow, RenderedQuestion, RowSpan};

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
    /// in [`Decider::run`], against [`LoadedModel::n_ctx`].
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

/// A validated, tokenized request, ready to run.
#[derive(Debug, Clone)]
pub struct Plan {
    rqs: IndexMap<String, RenderedQuestion>,
    index: Vec<RowSpan>,
    rows: Vec<PlannedRow>,
    items: Vec<Item>,
    input_tokens: usize,
}

impl Plan {
    /// Scoring rows (one decode each).
    pub fn rows(&self) -> usize {
        self.items.len()
    }

    /// Tokens of the longest row; 0 without rows.
    pub fn longest_row(&self) -> usize {
        self.items.iter().map(|it| it.ids.len()).max().unwrap_or(0)
    }

    /// Tokens over all rows.
    pub fn total_tokens(&self) -> usize {
        self.items.iter().map(|it| it.ids.len()).sum()
    }

    /// The `usage.input_tokens` the response reports: the shared state counts once.
    pub fn input_tokens(&self) -> usize {
        self.input_tokens
    }

    /// The token ids of every row, in decode order.
    pub fn row_ids(&self) -> impl Iterator<Item = &[u32]> {
        self.items.iter().map(|it| it.ids.as_slice())
    }
}

/// Reads decider-format and stock models through one tokenizer and profile.
#[derive(Debug, Clone)]
pub struct Decider {
    tokenizer: Tokenizer,
    profile: ModelProfile,
    labels: LabelTable,
    limits: Limits,
}

impl Decider {
    pub fn new(tokenizer: Tokenizer, profile: ModelProfile) -> Result<Decider, DecideError> {
        let labels = LabelTable::new(&tokenizer)?;
        Ok(Decider {
            tokenizer,
            profile,
            labels,
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
    pub fn plan(&self, req: &SystemOneRequest) -> Result<Plan, DecideError> {
        let mut rqs = IndexMap::with_capacity(req.questions.len());
        for (id, spec) in &req.questions {
            let rq = systemone::render_question(spec).map_err(|msg| DecideError::Invalid {
                loc: vec!["body".into(), "questions".into(), id.clone()],
                msg,
            })?;
            rqs.insert(id.clone(), rq);
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
        };
        self.check(&plan, self.limits.context_window)?;
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

    /// Decodes every row alone and reads the answers: per slot, softmax over the question's label logits divided by
    /// the temperature of its answer type.
    pub fn run(
        &self,
        model: &mut dyn LoadedModel,
        plan: &Plan,
    ) -> Result<SystemOneResponse, DecideError> {
        self.check(plan, Some(model.n_ctx()))?;
        let mut probs = Vec::with_capacity(plan.items.len());
        for (item, row) in plan.items.iter().zip(&plan.rows) {
            let width = item.nopts.iter().copied().max().unwrap_or(0);
            let label_ids = &self.labels.ids[..width];
            let logits = model
                .slot_logits(&item.ids, &item.slots, label_ids)
                .map_err(DecideError::Runtime)?;
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
        Ok(SystemOneResponse {
            model: self.profile.name.clone(),
            answers: systemone::assemble(&plan.rqs, &plan.index, &probs),
            usage: Usage {
                input_tokens: plan.input_tokens as u64,
                output_tokens: 0,
            },
        })
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
