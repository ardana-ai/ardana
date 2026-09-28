//! Reading a `/v1/systemone` body the way decider's FastAPI server does: a JSON body (read as JSON when the
//! `Content-Type` is absent, `application/json` or `application/*+json`), an object with `state` and `questions`, a
//! `questions` object and an optional string `model`. Each failure is a 422 entry with pydantic's type and message;
//! the question specs themselves are validated later by [`ardana_core::Decider::plan`].

use ardana_api::SystemOneRequest;
use axum::http::HeaderValue;
use serde_json::{Map, Value, json};

use crate::error::{ApiError, ValidationItem};

/// The request `body`, sent with `content_type`.
pub fn parse(
    content_type: Option<&HeaderValue>,
    body: &[u8],
) -> Result<SystemOneRequest, ApiError> {
    let invalid = |item: ValidationItem| ApiError::Validation(vec![item]);
    if body.is_empty() {
        return Err(invalid(ValidationItem::new(
            vec![json!("body")],
            "Field required",
            "missing",
        )));
    }
    if !is_json(content_type) {
        return Err(invalid(not_an_object()));
    }
    let value: Value = serde_json::from_slice(body).map_err(|err| {
        let mut item = ValidationItem::new(
            vec![json!("body"), json!(position(body, &err))],
            "JSON decode error",
            "json_invalid",
        );
        item.ctx = Some(json!({ "error": err.to_string() }));
        invalid(item)
    })?;
    let Value::Object(fields) = &value else {
        return Err(invalid(not_an_object()));
    };
    let errors = check_fields(fields);
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }
    serde_json::from_value(value).map_err(|err| {
        invalid(ValidationItem::new(
            vec![json!("body")],
            err.to_string(),
            "value_error",
        ))
    })
}

/// Whether the body is read as JSON: no `Content-Type`, `application/json` or `application/<x>+json` (FastAPI's
/// rule); any other type leaves the body raw bytes, which are not an object.
fn is_json(content_type: Option<&HeaderValue>) -> bool {
    let Some(value) = content_type else {
        return true;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let mime = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match mime.split_once('/') {
        Some(("application", subtype)) => subtype == "json" || subtype.ends_with("+json"),
        _ => false,
    }
}

fn not_an_object() -> ValidationItem {
    ValidationItem::new(
        vec![json!("body")],
        "Input should be a valid dictionary or object to extract fields from",
        "model_attributes_type",
    )
}

/// pydantic's errors for the top-level fields, in field order.
fn check_fields(fields: &Map<String, Value>) -> Vec<ValidationItem> {
    let loc = |field: &str| vec![json!("body"), json!(field)];
    let mut errors = Vec::new();
    if !fields.contains_key("state") {
        errors.push(ValidationItem::new(
            loc("state"),
            "Field required",
            "missing",
        ));
    }
    match fields.get("questions") {
        None => errors.push(ValidationItem::new(
            loc("questions"),
            "Field required",
            "missing",
        )),
        Some(Value::Object(_)) => {}
        Some(_) => errors.push(ValidationItem::new(
            loc("questions"),
            "Input should be a valid dictionary",
            "dict_type",
        )),
    }
    match fields.get("model") {
        None | Some(Value::Null | Value::String(_)) => {}
        Some(_) => errors.push(ValidationItem::new(
            loc("model"),
            "Input should be a valid string",
            "string_type",
        )),
    }
    errors
}

/// The character offset in `body` where parsing stopped, as FastAPI reports Python's `JSONDecodeError.pos`: the
/// offending character (serde_json's one-based `line` and byte `column`), or the end of an incomplete body.
fn position(body: &[u8], err: &serde_json::Error) -> usize {
    let end = if err.is_eof() {
        body.len()
    } else {
        let line_start: usize = body
            .split(|&b| b == b'\n')
            .take(err.line().saturating_sub(1))
            .map(|l| l.len() + 1)
            .sum();
        (line_start + err.column().saturating_sub(1)).min(body.len())
    };
    String::from_utf8_lossy(&body[..end]).chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(err: ApiError) -> Vec<(Vec<Value>, String)> {
        match err {
            ApiError::Validation(items) => items.into_iter().map(|i| (i.loc, i.kind)).collect(),
            other => panic!("not a validation error: {other:?}"),
        }
    }

    #[test]
    fn content_types() {
        let json_type = HeaderValue::from_static("application/json; charset=utf-8");
        let problem = HeaderValue::from_static("application/problem+json");
        let text = HeaderValue::from_static("text/plain");
        let body = br#"{"state":"s","questions":{}}"#;
        assert!(parse(None, body).is_ok());
        assert!(parse(Some(&json_type), body).is_ok());
        assert!(parse(Some(&problem), body).is_ok());
        assert_eq!(
            items(parse(Some(&text), body).unwrap_err()),
            [(vec![json!("body")], "model_attributes_type".to_string())]
        );
    }

    #[test]
    fn field_errors_in_order() {
        assert_eq!(
            items(parse(None, br#"{"model":5,"questions":[]}"#).unwrap_err()),
            [
                (vec![json!("body"), json!("state")], "missing".to_string()),
                (
                    vec![json!("body"), json!("questions")],
                    "dict_type".to_string()
                ),
                (
                    vec![json!("body"), json!("model")],
                    "string_type".to_string()
                ),
            ]
        );
        assert_eq!(
            items(parse(None, b"").unwrap_err()),
            [(vec![json!("body")], "missing".to_string())]
        );
        assert_eq!(
            items(parse(None, b"[1]").unwrap_err()),
            [(vec![json!("body")], "model_attributes_type".to_string())]
        );
    }

    #[test]
    fn json_errors_carry_the_position() {
        assert_eq!(
            items(parse(None, "{\"state\": \"é\",\n \"questions\": {".as_bytes()).unwrap_err()),
            [(vec![json!("body"), json!(30)], "json_invalid".to_string())]
        );
        assert_eq!(
            items(parse(None, "{\"state\": \"é\",\n \"questions\": x}".as_bytes()).unwrap_err()),
            [(vec![json!("body"), json!(29)], "json_invalid".to_string())]
        );
    }
}
