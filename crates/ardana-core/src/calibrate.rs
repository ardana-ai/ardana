//! What `ardana finetune` does with its model once it is built: the temperatures fitted on the calibration split as
//! decider fits them (`decider/calibrate.py#fit_by_type`, Q30), and the metrics of a model's answers on the test
//! split as `decider/metrics.py` computes them (accuracy, NLL, Brier score and the 15-bin ECE).
//!
//! A [`Record`] is one labelled answer read at temperature 1: the label logits [`Decider::run`] read it from. At any
//! temperature it gives the probabilities the response computes from them ([`Record::at`]), so the fit minimizes
//! the NLL of the probabilities the response reports. A [`Graded`] answer is one the response gave, its probabilities
//! as reported, against its label.

use std::collections::BTreeMap;

use ardana_api::{Answer, SystemOneResponse};
use serde::Serialize;

use crate::engine::{DecideError, Decider, softmax};
use crate::finetune::Example;
use crate::profile::AnswerType;
use crate::py;
use crate::runtime::LoadedModel;
use crate::systemone::{combine_isolated, render_question, scaled};

/// A type with fewer calibration answers than this takes the temperature fitted over every answer (Q30).
pub const MIN_ANSWERS: usize = 50;
/// The bins of the expected calibration error.
pub const ECE_BINS: usize = 15;
/// The places a fitted temperature is rounded to, as decider writes them into `decider_config.json`.
const TEMPERATURE_PLACES: usize = 3;
/// The places a reported metric is rounded to, as the answers' probabilities are.
const METRIC_PLACES: usize = 4;
/// The smallest probability the NLL reads (`metrics.py`'s clip).
const MIN_PROBABILITY: f64 = 1e-12;
/// The temperatures the fit tries first: [`GRID_POINTS`] spaced evenly in log from the first to the second.
const GRID: (f64, f64) = (0.05, 20.0);
const GRID_POINTS: usize = 801;
/// The golden-section steps that refine the grid's best temperature between its neighbours.
const REFINE_STEPS: usize = 40;

/// One labelled answer read at temperature 1: the label logits of its row, or of each level's `no`/`yes` row for an
/// isolated score question, and the gold option's position.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub kind: AnswerType,
    pub rows: Vec<Vec<f32>>,
    pub isolated: bool,
    pub gold: usize,
}

impl Record {
    /// The answer at temperature `t`, its probabilities the ones the response computes before rounding them: the
    /// row's [`softmax`] at `t`, or each level's P(yes) combined ([`combine_isolated`]), [`scaled`] as reported.
    pub fn at(&self, t: f64) -> Graded {
        let p = if self.isolated {
            let fit: Vec<f64> = self.rows.iter().map(|row| softmax(row, t)[1]).collect();
            combine_isolated(&fit).0
        } else {
            softmax(&self.rows[0], t)
        };
        Graded {
            kind: self.kind,
            probabilities: scaled(&p),
            gold: self.gold,
        }
    }
}

/// A model that hands every decode on to `model` and keeps the label logits it returns, in decode order.
struct Recording<'a> {
    model: &'a mut dyn LoadedModel,
    rows: Vec<Vec<Vec<f32>>>,
}

impl LoadedModel for Recording<'_> {
    fn n_ctx(&self) -> usize {
        self.model.n_ctx()
    }

    fn slot_logits(
        &mut self,
        ids: &[u32],
        slots: &[usize],
        label_ids: &[u32],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let logits = self.model.slot_logits(ids, slots, label_ids)?;
        self.rows.push(logits.clone());
        Ok(logits)
    }
}

/// The records of `example`'s labelled questions, in plan order: its request planned and answered by `model` through
/// [`Decider::run`], each labelled row's label logits kept as the model returned them.
pub fn records(
    decider: &Decider,
    model: &mut dyn LoadedModel,
    example: &Example,
) -> Result<Vec<Record>, DecideError> {
    let plan = decider.plan(&example.request)?;
    let mut recording = Recording {
        model,
        rows: Vec::new(),
    };
    decider.run(&mut recording, &plan)?;
    let mut records: Vec<Record> = Vec::new();
    for (logits, (id, level)) in recording.rows.into_iter().zip(plan.row_questions()) {
        let Some(label) = example.labels.get(id) else {
            continue;
        };
        let row = logits
            .into_iter()
            .next()
            .expect("Decider::run reads one slot a row");
        if level.is_some_and(|level| level > 0) {
            records
                .last_mut()
                .expect("an isolated question's first level comes before the others")
                .rows
                .push(row);
            continue;
        }
        let rq = plan
            .question(id)
            .expect("a planned row's question is in its plan");
        let gold = label.option(rq).ok_or_else(|| DecideError::Invalid {
            loc: vec!["answers".into(), id.to_string()],
            msg: format!("{} is not one of its options", label.name()),
        })?;
        records.push(Record {
            kind: rq.kind,
            rows: vec![row],
            isolated: level.is_some(),
            gold,
        });
    }
    Ok(records)
}

/// The mean NLL of `records`' gold options at temperature `t`.
pub fn mean_nll(records: &[&Record], t: f64) -> f64 {
    py::sum(records.iter().map(|record| record.at(t).nll())) / records.len() as f64
}

/// The temperature with the lowest [`mean_nll`] of `records` (at least one): the best of the grid, refined by a
/// golden-section search in log between its neighbours, the grid's value kept when the refinement is no better
/// (`calibrate.py#fit`).
pub fn fit(records: &[&Record]) -> f64 {
    let (first, last) = (GRID.0.ln(), GRID.1.ln());
    let grid: Vec<f64> = (0..GRID_POINTS)
        .map(|i| (first + (last - first) * i as f64 / (GRID_POINTS - 1) as f64).exp())
        .collect();
    let values: Vec<f64> = grid.iter().map(|&t| mean_nll(records, t)).collect();
    let mut best = 0;
    for (i, &value) in values.iter().enumerate() {
        if value < values[best] {
            best = i;
        }
    }
    let nll = |log_t: f64| mean_nll(records, log_t.exp());
    let (mut lo, mut hi) = (
        grid[best.saturating_sub(1)].ln(),
        grid[(best + 1).min(GRID_POINTS - 1)].ln(),
    );
    let r = (5f64.sqrt() - 1.0) / 2.0;
    let (mut a, mut b) = (hi - r * (hi - lo), lo + r * (hi - lo));
    let (mut fa, mut fb) = (nll(a), nll(b));
    for _ in 0..REFINE_STEPS {
        if fa < fb {
            (hi, b, fb) = (b, a, fa);
            a = hi - r * (hi - lo);
            fa = nll(a);
        } else {
            (lo, a, fa) = (a, b, fb);
            b = lo + r * (hi - lo);
            fb = nll(b);
        }
    }
    let t = ((lo + hi) / 2.0).exp();
    if mean_nll(records, t) <= values[best] {
        t
    } else {
        grid[best]
    }
}

/// The temperatures Q30 fits: `temperature` over every calibration answer, and a `temperature_by_type` entry for
/// each type with at least [`MIN_ANSWERS`] of them, each rounded as `decider_config.json` holds it; a type left out
/// takes `temperature`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fitted {
    pub temperature: f64,
    pub temperature_by_type: BTreeMap<AnswerType, f64>,
    /// The calibration answers of each type.
    pub answers: BTreeMap<AnswerType, usize>,
}

/// `calibrate.py#fit_by_type` over `records`; `None` without records.
pub fn fit_by_type(records: &[Record]) -> Option<Fitted> {
    if records.is_empty() {
        return None;
    }
    let all: Vec<&Record> = records.iter().collect();
    let mut fitted = Fitted {
        temperature: py::round(fit(&all), TEMPERATURE_PLACES),
        temperature_by_type: BTreeMap::new(),
        answers: BTreeMap::new(),
    };
    for kind in AnswerType::ALL {
        let group: Vec<&Record> = records.iter().filter(|r| r.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        fitted.answers.insert(kind, group.len());
        if group.len() >= MIN_ANSWERS {
            fitted
                .temperature_by_type
                .insert(kind, py::round(fit(&group), TEMPERATURE_PLACES));
        }
    }
    Some(fitted)
}

/// One answer against its label: its probabilities in option order (a noul's `no` first) and the gold option's
/// position.
#[derive(Debug, Clone, PartialEq)]
pub struct Graded {
    pub kind: AnswerType,
    pub probabilities: Vec<f64>,
    pub gold: usize,
}

impl Graded {
    /// The option answered: a noul's `yes` at P(yes) ≥ 0.5, else the first most probable option.
    pub fn predicted(&self) -> usize {
        let p = &self.probabilities;
        if self.kind == AnswerType::Noul {
            return usize::from(p[1] >= 0.5);
        }
        let mut best = 0;
        for (i, &x) in p.iter().enumerate() {
            if x > p[best] {
                best = i;
            }
        }
        best
    }

    pub fn correct(&self) -> bool {
        self.predicted() == self.gold
    }

    /// The probability of the option answered.
    fn confidence(&self) -> f64 {
        self.probabilities[self.predicted()]
    }

    fn nll(&self) -> f64 {
        -self.probabilities[self.gold]
            .clamp(MIN_PROBABILITY, 1.0)
            .ln()
    }

    /// The squared distance of the probabilities from the gold option's one-hot.
    fn brier(&self) -> f64 {
        py::sum(self.probabilities.iter().enumerate().map(|(i, &p)| {
            let hit = if i == self.gold { 1.0 } else { 0.0 };
            (p - hit) * (p - hit)
        }))
    }
}

/// The answers `response` gives `example`'s labelled questions, graded against their labels, in label order: a
/// choice and a score answer by their reported probabilities, a noul by its P(yes).
pub fn graded(example: &Example, response: &SystemOneResponse) -> Result<Vec<Graded>, DecideError> {
    let mut graded = Vec::with_capacity(example.labels.len());
    for (id, label) in &example.labels {
        let rq = render_question(&example.request.questions[id]).map_err(|msg| {
            DecideError::Invalid {
                loc: vec!["questions".into(), id.clone()],
                msg,
            }
        })?;
        let answer = response.answers.get(id).ok_or_else(|| {
            DecideError::Runtime(anyhow::anyhow!("the response answers no question {id}"))
        })?;
        let probabilities: Vec<f64> = match answer {
            Answer::Choice(choice) => choice.probabilities.values().copied().collect(),
            Answer::Score(score) => score.probabilities.values().copied().collect(),
            Answer::Noul(noul) => vec![1.0 - noul.noul, noul.noul],
        };
        let gold = label.option(&rq).ok_or_else(|| DecideError::Invalid {
            loc: vec!["answers".into(), id.clone()],
            msg: format!("{} is not one of its options", label.name()),
        })?;
        graded.push(Graded {
            kind: rq.kind,
            probabilities,
            gold,
        });
    }
    Ok(graded)
}

/// `metrics.py#summarize` of some answers: how many, the share answered correctly ([`Graded::predicted`]), the mean
/// NLL of the gold option (its probability clipped to 1e-12), the mean Brier score and the expected calibration error
/// over [`ECE_BINS`] equal bins of the answered option's probability.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Metrics {
    pub answers: usize,
    pub accuracy: f64,
    pub nll: f64,
    pub brier: f64,
    pub ece: f64,
}

impl Metrics {
    /// The metrics of `graded` (at least one answer).
    pub fn of(graded: &[&Graded]) -> Metrics {
        let n = graded.len() as f64;
        let mean = |f: &dyn Fn(&Graded) -> f64| py::sum(graded.iter().map(|g| f(g))) / n;
        let correct = |g: &Graded| if g.correct() { 1.0 } else { 0.0 };
        let mut ece = 0.0;
        for bin in 0..ECE_BINS {
            let step = 1.0 / ECE_BINS as f64;
            let (lo, hi) = (bin as f64 * step, (bin + 1) as f64 * step);
            let hi = if bin + 1 == ECE_BINS { 1.0 } else { hi };
            let inside: Vec<&Graded> = graded
                .iter()
                .copied()
                .filter(|g| g.confidence() > lo && g.confidence() <= hi)
                .collect();
            if inside.is_empty() {
                continue;
            }
            let m = inside.len() as f64;
            let confidence = py::sum(inside.iter().map(|g| g.confidence())) / m;
            let accuracy = py::sum(inside.iter().map(|g| correct(g))) / m;
            ece += m / n * (confidence - accuracy).abs();
        }
        Metrics {
            answers: graded.len(),
            accuracy: mean(&correct),
            nll: mean(&Graded::nll),
            brier: mean(&Graded::brier),
            ece,
        }
    }

    /// Every metric rounded to four places, as the report gives them.
    fn rounded(self) -> Metrics {
        let round = |x: f64| py::round(x, METRIC_PLACES);
        Metrics {
            answers: self.answers,
            accuracy: round(self.accuracy),
            nll: round(self.nll),
            brier: round(self.brier),
            ece: round(self.ece),
        }
    }
}

/// The key of the metrics over every answer in a [`report`].
pub const OVERALL: &str = "overall";

/// The metrics of `graded` for each answer type it holds (`choice`, `noul`, `score`, in that order), then over every
/// answer ([`OVERALL`]), each rounded to four places.
pub fn report(graded: &[Graded]) -> Vec<(String, Metrics)> {
    let mut report = Vec::new();
    for kind in AnswerType::ALL {
        let of_kind: Vec<&Graded> = graded.iter().filter(|g| g.kind == kind).collect();
        if !of_kind.is_empty() {
            report.push((kind.to_string(), Metrics::of(&of_kind).rounded()));
        }
    }
    if !graded.is_empty() {
        let all: Vec<&Graded> = graded.iter().collect();
        report.push((OVERALL.to_string(), Metrics::of(&all).rounded()));
    }
    report
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ardana_api::SystemOneRequest;
    use serde_json::{Value, json};

    use super::*;
    use crate::finetune::Label;
    use crate::profile::{Layout, from_decider_config};

    /// SplitMix64's next value of `state`, as a float in [0, 1).
    fn uniform(state: &mut u64) -> f64 {
        *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Label logits of `n` options that put the gold one ahead by `margin` on average with noise `noise` around it, so
    /// read at temperature 1 they are as overconfident (a margin larger than the noise warrants) as asked.
    fn logits(state: &mut u64, n: usize, gold: usize, margin: f64, noise: f64) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let signal = if i == gold { margin } else { 0.0 };
                (signal + noise * (uniform(state) - 0.5)) as f32
            })
            .collect()
    }

    /// Records of every kind, each read wrong now and then: 120 choices of 4 options (overconfident), 30 nouls (too
    /// few for a temperature of their own) and 60 isolated score questions of 5 levels (underconfident).
    fn calibration_records() -> Vec<Record> {
        let mut state = 7;
        let mut records = Vec::new();
        for i in 0..120 {
            let gold = i % 4;
            let read = if uniform(&mut state) < 0.7 {
                gold
            } else {
                (gold + 1) % 4
            };
            records.push(Record {
                kind: AnswerType::Choice,
                rows: vec![logits(&mut state, 4, read, 8.0, 2.0)],
                isolated: false,
                gold,
            });
        }
        for i in 0..30 {
            let gold = i % 2;
            let read = if uniform(&mut state) < 0.8 {
                gold
            } else {
                1 - gold
            };
            records.push(Record {
                kind: AnswerType::Noul,
                rows: vec![logits(&mut state, 2, read, 3.0, 1.0)],
                isolated: false,
                gold,
            });
        }
        for i in 0..60 {
            let gold = i % 5;
            let read = if uniform(&mut state) < 0.9 {
                gold
            } else {
                (gold + 2) % 5
            };
            let rows = (0..5)
                .map(|level| logits(&mut state, 2, usize::from(level == read), 0.6, 0.4))
                .collect();
            records.push(Record {
                kind: AnswerType::Score,
                rows,
                isolated: true,
                gold,
            });
        }
        records
    }

    /// R2.1: Q30's temperatures, each fitted on its records, minimize the NLL of the probabilities the response
    /// reports for the gold answers: at each fitted value it is at most the NLL at 0.05 either side and at 1.0. A
    /// type with 50 answers or more gets a temperature of its own, the nouls (30) take the one over every answer.
    #[test]
    fn fitted_temperatures_minimize_nll() {
        let records = calibration_records();
        let fitted = fit_by_type(&records).unwrap();
        assert_eq!(
            fitted.answers,
            BTreeMap::from([
                (AnswerType::Choice, 120),
                (AnswerType::Noul, 30),
                (AnswerType::Score, 60)
            ])
        );
        assert_eq!(
            fitted.temperature_by_type.keys().collect::<Vec<_>>(),
            [&AnswerType::Choice, &AnswerType::Score]
        );
        let all: Vec<&Record> = records.iter().collect();
        let of = |kind: AnswerType| -> Vec<&Record> {
            records.iter().filter(|r| r.kind == kind).collect()
        };
        let choice = fitted.temperature_by_type[&AnswerType::Choice];
        let score = fitted.temperature_by_type[&AnswerType::Score];
        // Overconfident choices are cooled, underconfident levels sharpened.
        assert!(choice > 1.5, "{choice}");
        assert!(score < 0.8, "{score}");
        for (group, t) in [
            (all, fitted.temperature),
            (of(AnswerType::Choice), choice),
            (of(AnswerType::Score), score),
        ] {
            let nll = mean_nll(&group, t);
            for other in [t - 0.05, t + 0.05, 1.0] {
                assert!(
                    nll <= mean_nll(&group, other),
                    "NLL {nll} at {t}, {} at {other}",
                    mean_nll(&group, other)
                );
            }
        }
        assert_eq!(fit_by_type(&[]), None);
    }

    /// The pinned snapshot directory of `repo` in `$HF_HOME/hub` (cargo's `[env]` points it at `tmp/hf`).
    fn snapshot(repo: &str) -> PathBuf {
        let dir = PathBuf::from(std::env::var_os("HF_HOME").expect("cargo sets HF_HOME"))
            .join("hub")
            .join(format!("models--{}", repo.replace('/', "--")));
        let commit = std::fs::read_to_string(dir.join("refs/main"))
            .unwrap_or_else(|err| panic!("{}: {err}; run `cargo xtask fetch`", dir.display()));
        dir.join("snapshots").join(commit.trim())
    }

    /// decider:2b's tokenizer in its own profile, plain with isolated levels, at the given temperatures.
    fn decider_2b(temperatures: Value) -> Decider {
        let dir = snapshot("Mapika/decider-2b-GGUF");
        let mut config: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("decider_config.json")).unwrap(),
        )
        .unwrap();
        config["temperature"] = temperatures["temperature"].clone();
        config["temperature_by_type"] = temperatures["temperature_by_type"].clone();
        let profile = from_decider_config(&config, Layout::Plain).unwrap();
        let path = dir.join("tokenizer.json");
        let tokenizer = tokenizers::Tokenizer::from_file(&path)
            .unwrap_or_else(|err| panic!("{}: {err}; run `cargo xtask fetch`", path.display()));
        Decider::new(tokenizer, profile).unwrap()
    }

    /// A causal model whose label logits are a function of the row it decodes, the same whenever it decodes it.
    struct Hashed;

    impl LoadedModel for Hashed {
        fn n_ctx(&self) -> usize {
            40_960
        }

        fn slot_logits(
            &mut self,
            ids: &[u32],
            slots: &[usize],
            label_ids: &[u32],
        ) -> anyhow::Result<Vec<Vec<f32>>> {
            let mut state = ids
                .iter()
                .fold(0u64, |h, &id| h.wrapping_mul(31) ^ u64::from(id));
            Ok(slots
                .iter()
                .map(|_| {
                    label_ids
                        .iter()
                        .map(|_| (6.0 * uniform(&mut state) - 3.0) as f32)
                        .collect()
                })
                .collect())
        }
    }

    /// A record at a temperature is the answer the response gives at it: the same probabilities, to the four places
    /// the response rounds them to, for a choice, a noul, a score read listwise and a score read level by level, and
    /// the gold option of its label; an unlabelled question has none. The metrics follow `metrics.py`.
    #[test]
    fn records_read_what_the_response_reports() {
        let questions = json!({
            "team": {"type": "choice", "instructions": "Which team should handle it?", "criteria": ["billing", "sales", "tech"]},
            "refund": {"type": "noul", "instructions": "Does the customer ask for a refund?"},
            "mood": {"type": "score", "instructions": "How upset is the customer?", "criteria": ["calm", "annoyed", "furious"]},
            "urgency": {"type": "score", "instructions": "How urgent is it?", "criteria": {"1": "whenever", "5": "now", "3": "today"}, "isolated": false},
            "order": {"type": "noul", "instructions": "Is an order number given?"},
        });
        let example = Example {
            line: 1,
            request: SystemOneRequest {
                model: None,
                state: json!("I was charged twice for order A-104 and nobody answers."),
                questions: serde_json::from_value(questions).unwrap(),
            },
            labels: [
                ("team", Label::Choice("tech".into())),
                ("refund", Label::Noul(true)),
                ("mood", Label::Score("1".into())),
                ("urgency", Label::Score("5".into())),
            ]
            .into_iter()
            .map(|(id, label)| (id.to_string(), label))
            .collect(),
        };
        let at_one = decider_2b(json!({"temperature": 1.0, "temperature_by_type": {}}));
        let records = records(&at_one, &mut Hashed, &example).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|r| (r.kind, r.rows.len(), r.isolated, r.gold))
                .collect::<Vec<_>>(),
            [
                (AnswerType::Choice, 1, false, 2),
                (AnswerType::Noul, 1, false, 1),
                (AnswerType::Score, 3, true, 1),
                (AnswerType::Score, 1, false, 2),
            ]
        );

        let temperatures =
            json!({"temperature": 1.3, "temperature_by_type": {"choice": 0.7, "score": 2.4}});
        let decider = decider_2b(temperatures);
        let response = decider.decide(&mut Hashed, &example.request).unwrap();
        let answered = graded(&example, &response).unwrap();
        assert_eq!(answered.len(), records.len());
        for (record, answer) in records.iter().zip(&answered) {
            let read = record.at(decider.profile().temperature_for(record.kind));
            assert_eq!((read.kind, read.gold), (answer.kind, answer.gold));
            for (p, reported) in read.probabilities.iter().zip(&answer.probabilities) {
                assert!((p - reported).abs() <= 5e-5, "{read:?} {answer:?}");
            }
        }

        // `metrics.py#summarize`: a noul is answered yes from 0.5 on; the bins hold confidences in (lo, hi].
        let graded = [
            Graded {
                kind: AnswerType::Noul,
                probabilities: vec![0.5, 0.5],
                gold: 1,
            },
            Graded {
                kind: AnswerType::Choice,
                probabilities: vec![0.2, 0.7, 0.1],
                gold: 0,
            },
        ];
        let metrics = Metrics::of(&graded.iter().collect::<Vec<_>>());
        assert_eq!(metrics.answers, 2);
        assert_eq!(metrics.accuracy, 0.5);
        assert!((metrics.nll - (-(0.5f64.ln()) - 0.2f64.ln()) / 2.0).abs() < 1e-12);
        assert!((metrics.brier - (0.5 + (0.64 + 0.49 + 0.01)) / 2.0).abs() < 1e-12);
        assert!((metrics.ece - (0.5 * 0.5 + 0.5 * 0.7)).abs() < 1e-12);
        let report = report(&graded);
        assert_eq!(
            report
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            ["choice", "noul", OVERALL]
        );
        assert_eq!(report[2].1.nll, py::round(metrics.nll, 4));
    }
}
