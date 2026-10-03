//! Error bodies, as the server answers them and the playground's in-tab engine reproduces them byte for byte: FastAPI's
//! 422 list (`{"detail": [{"loc", "msg", "type"}]}`), TypeSafe's shape for every other error
//! (`{"detail": {"error_type", "message"}}`) and FastAPI's text of an unknown path or method (`{"detail": "Not Found"}`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One entry of a 422 `detail` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationItem {
    /// The offending part of the request, starting with `"body"`.
    pub loc: Vec<Value>,
    pub msg: String,
    /// The pydantic error type (`missing`, `json_invalid`, `value_error`, ...).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx: Option<Value>,
}

impl ValidationItem {
    pub fn new(loc: Vec<Value>, msg: impl Into<String>, kind: &str) -> ValidationItem {
        ValidationItem {
            loc,
            msg: msg.into(),
            kind: kind.to_string(),
            ctx: None,
        }
    }
}

/// An error response body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub detail: Detail,
}

/// What an error body says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Detail {
    /// 422: every problem with the request.
    Validation(Vec<ValidationItem>),
    /// Every other error.
    Error { error_type: String, message: String },
    /// FastAPI's own 404 and 405 (`Not Found`, `Method Not Allowed`).
    Text(String),
}

impl ErrorBody {
    /// A 422 body.
    pub fn validation(items: Vec<ValidationItem>) -> ErrorBody {
        ErrorBody {
            detail: Detail::Validation(items),
        }
    }

    /// TypeSafe's error body: `error_type` (`request_too_large`, `api_error`, ...) and a message.
    pub fn error(error_type: &str, message: impl Into<String>) -> ErrorBody {
        ErrorBody {
            detail: Detail::Error {
                error_type: error_type.to_string(),
                message: message.into(),
            },
        }
    }

    /// FastAPI's body of an unknown path or method.
    pub fn text(text: &str) -> ErrorBody {
        ErrorBody {
            detail: Detail::Text(text.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bodies_serialise_as_the_api_answers() {
        let mut item = ValidationItem::new(
            vec![json!("body"), json!(3)],
            "JSON decode error",
            "json_invalid",
        );
        let body = ErrorBody::validation(vec![item.clone()]);
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"detail":[{"loc":["body",3],"msg":"JSON decode error","type":"json_invalid"}]}"#
        );
        item.ctx = Some(json!({"error": "EOF"}));
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            json!({"loc": ["body", 3], "msg": "JSON decode error", "type": "json_invalid", "ctx": {"error": "EOF"}})
        );
        assert_eq!(
            serde_json::to_string(&ErrorBody::error("request_too_large", "too many tokens"))
                .unwrap(),
            r#"{"detail":{"error_type":"request_too_large","message":"too many tokens"}}"#
        );
        assert_eq!(
            serde_json::to_string(&ErrorBody::text("Not Found")).unwrap(),
            r#"{"detail":"Not Found"}"#
        );
        for body in [
            ErrorBody::validation(vec![item]),
            ErrorBody::error("api_error", "x"),
            ErrorBody::text("x"),
        ] {
            let text = serde_json::to_string(&body).unwrap();
            assert_eq!(
                serde_json::from_str::<ErrorBody>(&text).unwrap(),
                body,
                "{text}"
            );
        }
    }
}
