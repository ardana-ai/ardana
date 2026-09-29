//! The request the playground sends, built from the editors, and the reply it reads back.

use ardana_api::{Answer, SystemOneRequest};
use indexmap::IndexMap;
use serde_json::Value;

/// A `questions` map: question id to its raw spec, in editor order.
pub type Questions = IndexMap<String, Value>;

/// State text that parses as a JSON object or array is sent as that JSON; any other text is sent as a string.
pub fn state_value(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(value @ (Value::Object(_) | Value::Array(_))) => value,
        _ => Value::String(text.to_string()),
    }
}

/// Parses the `questions` editor: a JSON object of question specs. Errors say where the text goes wrong in plain
/// words.
pub fn parse_questions(text: &str) -> Result<Questions, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map.into_iter().collect()),
        Ok(_) => Err("Questions JSON must be an object of question ids to questions".into()),
        Err(err) => {
            let at = format!("line {}, column {}", err.line(), err.column());
            Err(match err.classify() {
                serde_json::error::Category::Eof => format!("Questions JSON ends early at {at}"),
                _ => {
                    let what = err.to_string();
                    let what = what.split(" at line ").next().unwrap_or_default();
                    format!("Questions JSON has an error at {at}: {what}")
                }
            })
        }
    }
}

/// The request the editors describe; no model (empty) leaves the choice to the server.
pub fn request(model: &str, state_text: &str, questions: &Questions) -> SystemOneRequest {
    SystemOneRequest {
        model: (!model.is_empty()).then(|| model.to_string()),
        state: state_value(state_text),
        questions: questions.clone(),
    }
}

/// The exact body the playground sends, pretty-printed so the raw panel reads well.
pub fn body(request: &SystemOneRequest) -> String {
    serde_json::to_string_pretty(request).expect("a request of JSON values serialises")
}

/// The editor texts of a request, as Jev writes them into a share link: the state as it is when it is a string,
/// else pretty-printed JSON, and the questions pretty-printed.
pub fn editor_texts(request: &SystemOneRequest) -> (String, String) {
    let state = match &request.state {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).expect("JSON values serialise"),
    };
    (state, questions_text(&request.questions))
}

/// The questions editor text of a map: pretty-printed with two-space indents, as `JSON.stringify(q, null, 2)`.
pub fn questions_text(questions: &Questions) -> String {
    serde_json::to_string_pretty(questions).expect("JSON values serialise")
}

/// One answer as the playground shows it: typed when it is one of the three Jev types, else its raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    Typed(Box<Answer>),
    Raw(Value),
}

/// A `/v1/systemone` reply, read from its status and body text.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Answered {
        model: String,
        answers: IndexMap<String, Shown>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    /// A non-200 status with the body's `detail`, or the body text when it has none.
    Failed { status: u16, detail: Detail },
}

/// An error body's `detail` as the API returned it.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    /// 422: `[{"loc","msg","type"}]`.
    Validation(Vec<Issue>),
    /// Every other error: `{"error_type","message"}`.
    Error { error_type: String, message: String },
    /// Any other `detail` value, or a body that is not JSON.
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub loc: Vec<String>,
    pub msg: String,
}

pub fn read_reply(status: u16, body: &str) -> Reply {
    let json: Option<Value> = serde_json::from_str(body).ok();
    if status == 200
        && let Some(Value::Object(root)) = &json
        && let Some(Value::Object(answers)) = root.get("answers")
    {
        let usage = root.get("usage");
        let tokens = |key: &str| usage.and_then(|u| u.get(key)).and_then(Value::as_u64);
        return Reply::Answered {
            model: root
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            answers: answers
                .iter()
                .map(|(id, answer)| {
                    let shown = serde_json::from_value(answer.clone()).map_or_else(
                        |_| Shown::Raw(answer.clone()),
                        |a| Shown::Typed(Box::new(a)),
                    );
                    (id.clone(), shown)
                })
                .collect(),
            input_tokens: tokens("input_tokens"),
            output_tokens: tokens("output_tokens"),
        };
    }
    let detail = match json.as_ref().and_then(|j| j.get("detail")) {
        Some(Value::Array(items)) => Detail::Validation(
            items
                .iter()
                .map(|item| Issue {
                    loc: item["loc"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|part| match part {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect(),
                    msg: item["msg"]
                        .as_str()
                        .map_or_else(|| item.to_string(), str::to_string),
                })
                .collect(),
        ),
        Some(Value::Object(obj)) if obj.contains_key("message") => Detail::Error {
            error_type: obj
                .get("error_type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            message: obj
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| obj["message"].to_string(), str::to_string),
        },
        Some(Value::String(text)) => Detail::Other(text.clone()),
        Some(other) => Detail::Other(other.to_string()),
        None => Detail::Other(body.to_string()),
    };
    Reply::Failed { status, detail }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_objects_and_arrays_are_sent_as_json() {
        assert_eq!(state_value(r#"{"a": 1}"#), json!({"a": 1}));
        assert_eq!(state_value("[1, 2]"), json!([1, 2]));
        assert_eq!(state_value("42"), json!("42"));
        assert_eq!(state_value("\"quoted\""), json!("\"quoted\""));
        assert_eq!(
            state_value("My card was charged"),
            json!("My card was charged")
        );
    }

    #[test]
    fn questions_keep_their_order() {
        let q = parse_questions(r#"{"z": {"type": "noul"}, "a": {"type": "choice"}}"#).unwrap();
        assert_eq!(q.keys().collect::<Vec<_>>(), ["z", "a"]);
        assert!(
            parse_questions("[]")
                .unwrap_err()
                .contains("must be an object")
        );
        assert_eq!(
            parse_questions(r#"{"refund": "#).unwrap_err(),
            "Questions JSON ends early at line 1, column 11"
        );
        assert_eq!(
            parse_questions("{\n  \"a\" 1}").unwrap_err(),
            "Questions JSON has an error at line 2, column 7: expected `:`"
        );
    }

    #[test]
    fn body_is_the_request() {
        let q = parse_questions(r#"{"r": {"type": "noul", "instructions": "Refund?"}}"#).unwrap();
        let sent: Value = serde_json::from_str(&body(&request("decider-2b", "Hi", &q))).unwrap();
        assert_eq!(
            sent,
            json!({"model": "decider-2b", "state": "Hi", "questions": {"r": {"type": "noul", "instructions": "Refund?"}}})
        );
        // No model picked yet: the server's default answers.
        let sent: Value = serde_json::from_str(&body(&request("", "Hi", &q))).unwrap();
        assert_eq!(sent.get("model"), None);
    }

    #[test]
    fn editor_texts_are_jev_texts() {
        let req: SystemOneRequest = serde_json::from_str(
            r#"{"state":{"chat":[1]},"questions":{"b":{"type":"noul"},"a":{"type":"noul"}}}"#,
        )
        .unwrap();
        let (state, questions) = editor_texts(&req);
        assert_eq!(state, "{\n  \"chat\": [\n    1\n  ]\n}");
        assert!(questions.starts_with("{\n  \"b\": {\n    \"type\": \"noul\"\n  },"));
        let text = SystemOneRequest {
            state: json!("plain"),
            ..req
        };
        assert_eq!(editor_texts(&text).0, "plain");
    }

    #[test]
    fn unknown_answer_types_stay_raw() {
        let reply = read_reply(
            200,
            r#"{"model":"m","answers":{"a":{"type":"noul","noul":0.9},"b":{"type":"rank","order":[1]}},"usage":{"input_tokens":7,"output_tokens":2}}"#,
        );
        let Reply::Answered {
            answers,
            input_tokens,
            ..
        } = reply
        else {
            panic!("{reply:?}")
        };
        assert!(matches!(&answers["a"], Shown::Typed(a) if matches!(**a, Answer::Noul(_))));
        assert_eq!(answers["b"], Shown::Raw(json!({"type":"rank","order":[1]})));
        assert_eq!(input_tokens, Some(7));
    }

    #[test]
    fn error_details_are_read() {
        let reply = read_reply(
            422,
            r#"{"detail":[{"loc":["body","questions","q"],"msg":"question without instructions","type":"value_error"}]}"#,
        );
        assert_eq!(
            reply,
            Reply::Failed {
                status: 422,
                detail: Detail::Validation(vec![Issue {
                    loc: vec!["body".into(), "questions".into(), "q".into()],
                    msg: "question without instructions".into()
                }])
            }
        );
        let reply = read_reply(
            413,
            r#"{"detail":{"error_type":"request_too_large","message":"the state exceeds the context window"}}"#,
        );
        assert!(matches!(
            reply,
            Reply::Failed {
                status: 413,
                detail: Detail::Error { .. }
            }
        ));
        assert_eq!(
            read_reply(502, "Bad gateway"),
            Reply::Failed {
                status: 502,
                detail: Detail::Other("Bad gateway".into())
            }
        );
    }
}
