//! R2.4–R2.6: the readout reproduces decider's `tests/test_systemone.py` numbers and `decider/temperature.py`
//! precedence; isolated levels follow the per-question flag; answers have the Jev shape.

mod common;

use anyhow::Result;
use ardana_api::{Answer, SystemOneRequest};
use ardana_core::engine::softmax;
use ardana_core::systemone::{
    self, assemble, choice_confidence, format_answer, plan_rows, render_question, score_confidence,
};
use ardana_core::{Decider, Layout, LoadedModel, ModelProfile, from_decider_config, py};
use indexmap::IndexMap;
use serde_json::{Value, json};

/// A model whose logit for label `i` is `i * step` at every slot; it records every decoded row.
struct Fake {
    step: f32,
    rows: Vec<Vec<u32>>,
}

impl LoadedModel for Fake {
    fn n_ctx(&self) -> usize {
        40_960
    }

    fn slot_logits(
        &mut self,
        ids: &[u32],
        slots: &[usize],
        label_ids: &[u32],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        self.rows.push(ids.to_vec());
        Ok(slots
            .iter()
            .map(|_| (0..label_ids.len()).map(|i| i as f32 * self.step).collect())
            .collect())
    }
}

fn fake() -> Fake {
    Fake {
        step: 0.5,
        rows: Vec::new(),
    }
}

fn rendered(questions: Value) -> IndexMap<String, systemone::RenderedQuestion> {
    questions
        .as_object()
        .expect("questions object")
        .iter()
        .map(|(k, v)| (k.clone(), render_question(v).expect("valid question")))
        .collect()
}

fn request(questions: Value) -> SystemOneRequest {
    serde_json::from_value(json!({"state": "My card was charged twice.", "questions": questions}))
        .expect("request")
}

fn decider(profile: ModelProfile) -> Result<Decider> {
    Ok(Decider::new(common::decider_2b_tokenizer()?, profile)?)
}

#[test]
fn decider_goldens() -> Result<()> {
    // tests/test_systemone.py::test_plan_rows_isolates_score_levels_and_assembles_answers
    let rqs = rendered(json!({
        "team": {"type": "choice", "instructions": "Which team?", "criteria": {"billing": null, "tech": null}},
        "mood": {"type": "score", "instructions": "How angry?", "criteria": ["calm", "annoyed", "furious"]},
        "refund": {"type": "noul", "instructions": "Refund?"}}));
    let (rows, index) = plan_rows(&rqs, true);
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows[1].question,
        "How angry?\nProposed answer: calm\nDoes the proposed answer fit?"
    );
    let probs = [
        vec![0.2, 0.8],
        vec![0.9, 0.1],
        vec![0.5, 0.5],
        vec![0.6, 0.4],
        vec![0.05, 0.95],
    ];
    let out = assemble(&rqs, &index, &probs);
    let json = serde_json::to_value(&out)?;
    assert_eq!(json["team"]["choice"], "tech");
    assert_eq!(json["team"]["confidence"], 0.6);
    assert_eq!(json["team"]["x_p_max"], 0.8);
    assert_eq!(
        json["team"]["probabilities"],
        json!({"billing": 0.2, "tech": 0.8})
    );
    assert_eq!(json["refund"], json!({"type": "noul", "noul": 0.95}));
    let mood = &json["mood"];
    assert_eq!(mood["x_level_fit"], json!({"0": 0.1, "1": 0.5, "2": 0.4}));
    assert_eq!(mood["x_fit_mass"], 1.0);
    assert_eq!(mood["probabilities"], json!({"0": 0.1, "1": 0.5, "2": 0.4}));
    assert_eq!(mood["score"], 1.3);
    assert_eq!(mood["confidence"], 0.25);
    assert_eq!(mood["x_p_max"], 0.5);
    assert_eq!(
        mood["legend"],
        json!({"0": "calm", "1": "annoyed", "2": "furious"})
    );

    // TypeSafe's score and choice examples
    let score3 = rendered(
        json!({"s": {"type": "score", "instructions": "How much?", "criteria": ["l0", "l1", "l2"]}}),
    );
    let Answer::Score(a) = format_answer(&score3["s"], &[0.0, 0.95, 0.05]) else {
        panic!("score")
    };
    assert_eq!(
        (a.confidence, a.x_p_max, a.score),
        (0.925, Some(0.95), 1.05)
    );
    assert!((score_confidence(&[0.01, 0.02, 0.07, 0.3, 0.6]) - 0.55).abs() < 1e-9);
    assert!((score_confidence(&[0.5, 0.0, 0.5])).abs() < 1e-9);
    assert!((choice_confidence(&[0.88, 0.12]) - 0.76).abs() < 1e-9);
    assert_eq!(choice_confidence(&[0.0, 0.0]), 0.0);
    // issue #15's conformance report example
    let colours = rendered(
        json!({"c": {"type": "choice", "instructions": "Which colour?",
        "criteria": ["Red", "Green", "Blue", "Yellow", "Purple"]}}),
    );
    let Answer::Choice(c) = format_answer(&colours["c"], &[0.195, 0.1994, 0.2252, 0.2286, 0.1518])
    else {
        panic!("choice")
    };
    assert_eq!(
        (c.choice.as_str(), c.x_p_max, c.confidence),
        ("Yellow", Some(0.2286), 0.0358)
    );
    assert_eq!(systemone::certainty(&[1.0, 0.0, 0.0]), 1.0);
    assert!(systemone::certainty(&[1.0 / 3.0; 3]).abs() < 1e-9);

    // decider/temperature.py precedence: temperature_by_type per type, "temperature" for a missing type, 1.0 default
    let cfg = json!({"temperature": 1.5, "temperature_by_type": {"choice": 2.0, "noul": 0.5}});
    let profile = from_decider_config(&cfg, Layout::Plain)?;
    assert_eq!(
        profile
            .effective_temperatures()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [2.0, 0.5, 1.5]
    );
    assert_eq!(
        from_decider_config(&json!({}), Layout::Plain)?.temperature,
        1.0
    );
    let profile = ModelProfile {
        isolated_levels: true,
        ..profile
    };
    let d = decider(profile)?;
    let mut model = fake();
    let resp = d.decide(&mut model, &request(json!({
        "c": {"type": "choice", "instructions": "Which?", "criteria": ["a", "b", "c"]},
        "n": {"type": "noul", "instructions": "Refund?"},
        "s": {"type": "score", "instructions": "How bad?", "criteria": ["low", "high"], "isolated": false}})))?;
    let expect = |logits: &[f32], t: f64| -> Vec<f64> {
        softmax(logits, t)
            .into_iter()
            .map(|p| py::round(p, 4))
            .collect()
    };
    let Answer::Choice(c) = &resp.answers["c"] else {
        panic!("choice")
    };
    assert_eq!(
        c.probabilities.values().copied().collect::<Vec<_>>(),
        expect(&[0.0, 0.5, 1.0], 2.0)
    );
    let Answer::Noul(n) = &resp.answers["n"] else {
        panic!("noul")
    };
    assert_eq!(n.noul, expect(&[0.0, 0.5], 0.5)[1]);
    let Answer::Score(s) = &resp.answers["s"] else {
        panic!("score")
    };
    assert_eq!(
        s.probabilities.values().copied().collect::<Vec<_>>(),
        expect(&[0.0, 0.5], 1.5)
    );
    Ok(())
}

#[test]
fn isolated_flag() -> Result<()> {
    let iso = ModelProfile {
        isolated_levels: true,
        ..ModelProfile::stock("m", Layout::Plain)
    };
    let d = decider(iso)?;
    let score = |isolated: Option<bool>| {
        let mut spec = json!({"type": "score", "instructions": "How bad?", "criteria": ["fine", "bad", "awful"]});
        if let Some(flag) = isolated {
            spec["isolated"] = json!(flag);
        }
        request(json!({ "s": spec }))
    };

    let mut model = fake();
    let resp = d.decide(&mut model, &score(None))?;
    assert_eq!(model.rows.len(), 3, "one yes/no row per level by default");
    let Answer::Score(a) = &resp.answers["s"] else {
        panic!("score")
    };
    let fit = a
        .x_level_fit
        .as_ref()
        .expect("isolated levels report their fit");
    assert_eq!(fit.keys().collect::<Vec<_>>(), ["0", "1", "2"]);
    assert!(a.x_fit_mass.is_some());
    let text = d
        .tokenizer()
        .decode(&model.rows[1], false)
        .map_err(anyhow::Error::msg)?;
    assert!(text.ends_with("Question: How bad?\nProposed answer: bad\nDoes the proposed answer fit?\nOptions:\n(A) no\n(B) yes\nAnswer: ("), "{text}");

    let mut model = fake();
    let resp = d.decide(&mut model, &score(Some(false)))?;
    assert_eq!(
        model.rows.len(),
        1,
        "\"isolated\": false reads the levels listwise"
    );
    let Answer::Score(a) = &resp.answers["s"] else {
        panic!("score")
    };
    assert!(a.x_level_fit.is_none() && a.x_fit_mass.is_none());
    let text = d
        .tokenizer()
        .decode(&model.rows[0], false)
        .map_err(anyhow::Error::msg)?;
    assert!(
        text.ends_with("Options:\n(A) 0: fine\n(B) 1: bad\n(C) 2: awful\nAnswer: ("),
        "{text}"
    );

    // A profile without isolated levels reads every score question listwise.
    let listwise = decider(ModelProfile::stock("m", Layout::Plain))?;
    let mut model = fake();
    let resp = listwise.decide(&mut model, &score(Some(true)))?;
    assert_eq!(model.rows.len(), 1);
    let Answer::Score(a) = &resp.answers["s"] else {
        panic!("score")
    };
    assert!(a.x_level_fit.is_none());
    Ok(())
}

/// decider's `systemone.unique_tokens`: the longest common prefix of all rows counts once.
fn unique_tokens(rows: &[Vec<u32>]) -> usize {
    match rows {
        [] => 0,
        [one] => one.len(),
        [first, rest @ ..] => {
            let short = rows.iter().map(Vec::len).min().unwrap_or(0);
            let lcp = (0..short)
                .take_while(|&i| rest.iter().all(|r| r[i] == first[i]))
                .count();
            lcp + rows.iter().map(|r| r.len() - lcp).sum::<usize>()
        }
    }
}

#[test]
fn response_shape() -> Result<()> {
    let profile = ModelProfile {
        isolated_levels: true,
        ..ModelProfile::stock("decider-test", Layout::Plain)
    };
    let d = decider(profile)?;
    let mut model = fake();
    let req = request(json!({
        "zeta": {"type": "score", "instructions": "How bad?", "criteria": ["fine", null, {"level": 2}]},
        "alpha": {"type": "choice", "instructions": "Which?", "criteria": {"x": "the x", "y": null}},
        "mid": {"type": "noul", "instructions": "Refund?"}}));
    let resp = d.decide(&mut model, &req)?;
    assert_eq!(resp.model, "decider-test");
    assert_eq!(
        resp.answers.keys().collect::<Vec<_>>(),
        ["zeta", "alpha", "mid"]
    );
    assert_eq!(resp.usage.output_tokens, 0);
    assert_eq!(resp.usage.input_tokens as usize, unique_tokens(&model.rows));

    let text = serde_json::to_string(&resp)?;
    let v: Value = serde_json::from_str(&text)?;
    let keys = |a: &Value| {
        a.as_object()
            .expect("answer object")
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        keys(&v["answers"]["zeta"]),
        [
            "type",
            "score",
            "confidence",
            "legend",
            "probabilities",
            "x_p_max",
            "x_certainty",
            "x_level_fit",
            "x_fit_mass"
        ]
    );
    assert_eq!(
        keys(&v["answers"]["alpha"]),
        [
            "type",
            "choice",
            "confidence",
            "probabilities",
            "x_p_max",
            "x_certainty"
        ]
    );
    assert_eq!(keys(&v["answers"]["mid"]), ["type", "noul"]);
    assert_eq!(
        v["answers"]["zeta"]["legend"],
        json!({"0": "fine", "1": "null", "2": "{\"level\": 2}"})
    );
    assert!(!text.contains(":null"), "no null anywhere: {text}");
    assert_eq!(keys(&v), ["model", "answers", "usage"]);

    // No questions: empty answers and no input tokens, as decider answers them.
    let mut model = fake();
    let empty = d.decide(&mut model, &request(json!({})))?;
    assert!(empty.answers.is_empty() && model.rows.is_empty());
    assert_eq!(empty.usage.input_tokens, 0);
    Ok(())
}
