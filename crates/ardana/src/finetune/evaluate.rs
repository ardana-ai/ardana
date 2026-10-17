//! What a fine-tune does with its model once it is built (Q11, Q30): the temperatures fitted on the calibration split,
//! read through that model as `ardana run` reads it (a GGUF through llama.cpp, a checkpoint through Candle), then the
//! test split answered by the base as `ardana run <base>` answers it and by the model at its fitted temperatures, each
//! answer graded against its label ([`ardana_core::calibrate`]). The decodes run on a thread of their own and stop
//! between two lines once `stop` is raised.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use ardana_core::calibrate::{self, Fitted, Graded, Metrics};
use ardana_core::finetune::Example;
use ardana_core::{Decider, LoadOptions, Loaded};
use ardana_registry::ResolvedModel;
use serde_json::{Value, json};

use super::INTERRUPTED;
use crate::render;

/// `model`'s decider and weights, as `ardana run <name>` reads and loads them (the default window and offload), and
/// the id of the runtime that loaded them: the first that supports them, as [`ResolvedModel::load`] picks it.
fn load(model: &ResolvedModel) -> Result<(Decider, Loaded, &'static str)> {
    let decider = Decider::new(
        ardana_registry::load_tokenizer(&model.tokenizer)?,
        model.profile.clone(),
    )?;
    let runtimes = crate::runtimes();
    let loaded = model.load(&runtimes, &LoadOptions::default())?;
    let runtime = runtimes
        .for_weights(&model.weights)
        .context("the runtime that loaded the model")?
        .id();
    Ok((decider, loaded, runtime))
}

/// The temperatures Q30 fits for `model` on `examples`, the calibration split, read at temperature 1, and the runtime
/// they were read through.
pub fn calibrate(
    model: &ResolvedModel,
    examples: &[Example],
    stop: &AtomicBool,
) -> Result<(Fitted, &'static str)> {
    let (decider, loaded, runtime) = load(model)?;
    let Loaded::Causal(mut loaded) = loaded else {
        bail!(
            "{} is a decision model, which reads no prompt rows",
            model.name
        );
    };
    let mut records = Vec::new();
    for example in examples {
        if stop.load(Ordering::Relaxed) {
            bail!(INTERRUPTED);
        }
        records.extend(
            calibrate::records(&decider, loaded.as_mut(), example)
                .with_context(|| format!("reading calibration line {}", example.line))?,
        );
    }
    let fitted =
        calibrate::fit_by_type(&records).context("the calibration split labels no answer")?;
    Ok((fitted, runtime))
}

/// The answers `model` gives `examples`, as `ardana run <model>` gives them, graded against their labels.
pub fn answer(
    model: &ResolvedModel,
    examples: &[Example],
    stop: &AtomicBool,
) -> Result<Vec<Graded>> {
    let (decider, mut loaded, _) = load(model)?;
    let mut graded = Vec::new();
    for example in examples {
        if stop.load(Ordering::Relaxed) {
            bail!(INTERRUPTED);
        }
        let read = |what: &str| format!("{what} test line {} with {}", example.line, model.name);
        let plan = decider
            .plan(&example.request)
            .with_context(|| read("planning"))?;
        let response = decider
            .answer(&mut loaded, &plan)
            .with_context(|| read("answering"))?;
        graded.extend(calibrate::graded(example, &response).with_context(|| read("grading"))?);
    }
    Ok(graded)
}

/// What `finetune.json#report` holds: the test split's lines, and the base's and the model's metrics on it per
/// answer type and overall ([`calibrate::report`]).
pub struct Report {
    lines: usize,
    base: (String, Vec<(String, Metrics)>),
    tuned: (String, Vec<(String, Metrics)>),
}

impl Report {
    pub fn new(lines: usize, base: (&str, &[Graded]), tuned: (&str, &[Graded])) -> Report {
        Report {
            lines,
            base: (base.0.to_string(), calibrate::report(base.1)),
            tuned: (tuned.0.to_string(), calibrate::report(tuned.1)),
        }
    }

    /// `{"lines", "base": {"name", "metrics": {<type>: {"answers", "accuracy", "nll", "brier", "ece"}, "overall": ...}},
    /// "tuned": ...}`.
    pub fn to_json(&self) -> Value {
        let model = |(name, metrics): &(String, Vec<(String, Metrics)>)| {
            let metrics: serde_json::Map<String, Value> = metrics
                .iter()
                .map(|(key, m)| (key.clone(), json!(m)))
                .collect();
            json!({"name": name, "metrics": metrics})
        };
        json!({"lines": self.lines, "base": model(&self.base), "tuned": model(&self.tuned)})
    }

    /// The table stdout prints: a row per answer type and model, then the overall rows, the numbers as the JSON
    /// holds them.
    pub fn table(&self) -> String {
        let mut rows = Vec::new();
        for (kind, _) in &self.tuned.1 {
            for (name, metrics) in [&self.base, &self.tuned] {
                let Some((_, m)) = metrics.iter().find(|(key, _)| key == kind) else {
                    continue;
                };
                rows.push(vec![
                    kind.clone(),
                    name.clone(),
                    m.answers.to_string(),
                    format!("{:.4}", m.accuracy),
                    format!("{:.4}", m.nll),
                    format!("{:.4}", m.brier),
                    format!("{:.4}", m.ece),
                ]);
            }
        }
        format!(
            "{} against {} on the test split ({} lines):\n{}",
            self.tuned.0,
            self.base.0,
            self.lines,
            render::table(
                &[
                    "TYPE", "MODEL", "ANSWERS", "ACCURACY", "NLL", "BRIER", "ECE"
                ],
                &rows
            )
        )
    }
}
