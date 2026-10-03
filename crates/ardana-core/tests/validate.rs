//! R2.3: invalid questions fail with decider's messages at `["body","questions",<id>]`, and a state longer than the
//! context window is refused by `Decider::plan` instead of being truncated.

mod common;

use anyhow::Result;
use ardana_api::SystemOneRequest;
use ardana_core::{DecideError, Decider, Layout, Limits, ModelProfile};
use serde_json::{Value, json};

fn decider() -> Result<Decider> {
    Ok(Decider::new(
        common::decider_2b_tokenizer()?,
        ModelProfile::stock("m", Layout::Plain),
    )?)
}

fn request(state: Value, questions: Value) -> SystemOneRequest {
    serde_json::from_value(json!({"state": state, "questions": questions}))
        .expect("a valid request shape")
}

/// The `(loc, msg)` of the first invalid question.
fn invalid(d: &Decider, questions: Value) -> (Vec<String>, String) {
    match d.plan(&request(json!("a customer wants a refund"), questions)) {
        Err(DecideError::Invalid { loc, msg }) => (loc, msg),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn decider_messages() -> Result<()> {
    let d = decider()?;
    let one = |spec: Value| invalid(&d, json!({ "q": spec })).1;
    let choice = "choice criteria: a map of 2..255 options";
    assert_eq!(
        one(json!({"type": "choice", "instructions": "q", "criteria": {"only": null}})),
        choice
    );
    let wide: serde_json::Map<String, Value> =
        (0..256).map(|i| (i.to_string(), Value::Null)).collect();
    assert_eq!(
        one(json!({"type": "choice", "instructions": "q", "criteria": wide})),
        choice
    );
    assert_eq!(
        one(json!({"type": "choice", "instructions": "q", "criteria": "ab"})),
        choice
    );
    let score = "score criteria: an ordered list of 2..10 level descriptions";
    assert_eq!(
        one(json!({"type": "score", "instructions": "q", "criteria": ["one"]})),
        score
    );
    assert_eq!(
        one(json!({"type": "score", "instructions": "q", "criteria": (0..11).collect::<Vec<_>>()})),
        score
    );
    assert_eq!(one(json!({"type": "score", "instructions": "q"})), score);
    let noul = "noul question without instructions: criteria must describe true or false";
    assert_eq!(one(json!({"type": "noul"})), noul);
    assert_eq!(
        one(json!({"type": "noul", "criteria": {"true": null, "false": ""}})),
        noul
    );
    assert_eq!(one(json!({"type": "bool", "instructions": ""})), noul);
    assert_eq!(
        one(json!({"type": "noul", "criteria": ["bad"]})),
        "noul criteria: a map of optional true/false descriptions"
    );
    assert_eq!(
        one(json!({"type": "text", "instructions": "q"})),
        "unknown question type 'text'"
    );
    assert_eq!(
        one(json!({"type": null, "instructions": "q"})),
        "unknown question type None"
    );
    assert_eq!(
        one(json!({"type": "choice", "criteria": ["a", "b"]})),
        "question without instructions"
    );
    assert_eq!(
        one(json!({"type": "score", "instructions": "q", "criteria": {"x": "a", "1": "b"}})),
        "could not convert string to float: 'x'"
    );

    // The first invalid question in request order is reported, at its id.
    let (loc, msg) = invalid(
        &d,
        json!({"ok": {"type": "noul", "instructions": "fine?"}, "bad": {"type": "choice", "instructions": "q", "criteria": ["a"]},
               "worse": {"type": "nope"}}),
    );
    assert_eq!(loc, ["body", "questions", "bad"]);
    assert_eq!(msg, choice);

    // A question spec that is not an object is invalid input, never a panic.
    for spec in [
        json!("not an object"),
        json!(5),
        json!(null),
        json!(["a", "b"]),
    ] {
        let (loc, msg) = invalid(&d, json!({ "x": spec }));
        assert_eq!(loc, ["body", "questions", "x"]);
        assert!(msg.starts_with("question must be an object"), "{msg}");
    }

    // Valid specs in decider's other accepted forms.
    let ok = request(
        json!({"ticket": "A-104"}),
        json!({"a": {"type": "noul", "criteria": {"true": "money back"}},
               "b": {"type": "score", "instructions": "q", "criteria": {"1": "high", "0": "low"}},
               "c": {"instructions": "default type is choice", "options": ["x", "y"]}}),
    );
    assert_eq!(d.plan(&ok)?.rows(), 3);
    Ok(())
}

#[test]
fn context_window_is_a_capacity_error_not_a_truncation() -> Result<()> {
    let state = "refund ".repeat(400);
    let questions = json!({"q": {"type": "noul", "instructions": "Refund?"}});
    let base = decider()?;
    let full = base.plan(&request(json!(state), questions.clone()))?;
    assert!(full.longest_row() > 400, "the whole state is in the row");

    let small = base.clone().with_limits(Limits {
        context_window: Some(256),
        ..Limits::default()
    });
    match small.plan(&request(json!(state), questions.clone())) {
        Err(DecideError::Capacity(msg)) => {
            assert!(msg.contains("context window"), "{msg}");
            assert!(msg.contains(&full.longest_row().to_string()), "{msg}");
        }
        other => panic!("expected Capacity, got {other:?}"),
    }

    // The contract constructor alone checks the default load window (40,960 tokens).
    match base.plan(&request(json!(" word".repeat(41_000)), questions.clone())) {
        Err(DecideError::Capacity(msg)) => {
            assert!(msg.contains("context window of 40960 tokens"), "{msg}")
        }
        other => panic!("expected Capacity, got {other:?}"),
    }

    // decider's own limits, in its order.
    let rows = base.clone().with_limits(Limits {
        max_rows: 1,
        ..Limits::default()
    });
    let two = json!({"a": {"type": "noul", "instructions": "a?"}, "b": {"type": "noul", "instructions": "b?"}});
    match rows.plan(&request(json!("s"), two)) {
        Err(DecideError::Capacity(msg)) => assert_eq!(
            msg,
            "too many questions: the request expands to 2 scoring rows, the limit is 1"
        ),
        other => panic!("expected Capacity, got {other:?}"),
    }
    let row_tokens = base.clone().with_limits(Limits {
        max_row_tokens: 100,
        ..Limits::default()
    });
    assert!(matches!(
        row_tokens.plan(&request(json!(state), questions.clone())),
        Err(DecideError::Capacity(msg)) if msg.starts_with("too many tokens: one row has")
    ));
    let total = base.clone().with_limits(Limits {
        max_request_tokens: 100,
        ..Limits::default()
    });
    assert!(matches!(
        total.plan(&request(json!(state), questions)),
        Err(DecideError::Capacity(msg)) if msg.starts_with("too many tokens: the request has")
    ));
    Ok(())
}

/// What the API answers for each refusal: the server and the playground's in-tab engine both send these bodies.
#[test]
fn refusals_answer_as_the_api() -> Result<()> {
    let d = decider()?;
    let err = d
        .plan(&request(
            json!("s"),
            json!({"department": {"type": "choice", "instructions": "Which?", "criteria": ["billing"]}}),
        ))
        .expect_err("one option is refused");
    assert_eq!(err.status(), 422);
    assert_eq!(
        serde_json::to_string(&err.body())?,
        r#"{"detail":[{"loc":["body","questions","department"],"msg":"choice criteria: a map of 2..255 options","type":"value_error"}]}"#
    );

    let small = d.with_limits(Limits {
        max_rows: 1,
        ..Limits::default()
    });
    let err = small
        .plan(&request(
            json!("s"),
            json!({"a": {"type": "noul", "instructions": "a?"}, "b": {"type": "noul", "instructions": "b?"}}),
        ))
        .expect_err("two rows are refused");
    assert_eq!(err.status(), 413);
    assert_eq!(
        serde_json::to_string(&err.body())?,
        r#"{"detail":{"error_type":"request_too_large","message":"too many questions: the request expands to 2 scoring rows, the limit is 1"}}"#
    );

    let err = DecideError::Runtime(anyhow::anyhow!("decode failed").context("model m"));
    assert_eq!(err.status(), 500);
    assert_eq!(
        serde_json::to_string(&err.body())?,
        r#"{"detail":{"error_type":"api_error","message":"model m: decode failed"}}"#
    );
    Ok(())
}
