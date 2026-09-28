//! [`ApiError`]: the only place HTTP error statuses and bodies are built.
//!
//! Validation failures use FastAPI's shape, `{"detail": [{"loc", "msg", "type"}]}` with status 422, as decider and
//! TypeSafe answer them; every other error is `{"detail": {"error_type", "message"}}`, TypeSafe's live shape. Only a
//! failing runtime or model file is a 5xx: bad input never is.

use axum::Json;
use axum::extract::rejection::BytesRejection;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Value, json};

/// Seconds a client waits before retrying a 503.
const RETRY_AFTER_S: &str = "1";

/// One entry of a 422 `detail` list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationItem {
    /// The offending part of the request, starting with `"body"`.
    pub loc: Vec<Value>,
    pub msg: String,
    /// The pydantic error type (`missing`, `json_invalid`, `value_error`, ...).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// 422: the body is not a valid request.
    Validation(Vec<ValidationItem>),
    /// 413: the body, the rows or the tokens exceed a limit, or a row exceeds the context window.
    TooLarge(String),
    /// 404: the request names a model the registry does not have; the message lists the available names.
    UnknownModel(String),
    /// 404 for a path the API does not have.
    NotFound,
    /// 405 for a method a path does not take.
    MethodNotAllowed,
    /// 503 with `Retry-After`: accepting the request would exceed the queued-row limit.
    Busy(String),
    /// 403: the server needs a key and the request carries none.
    MissingKey,
    /// 401: the request carries a key that is not the server's.
    WrongKey,
    /// 400: the request body could not be read.
    BadBody(String),
    /// 500: the model files, the runtime or the model worker failed.
    Internal(String),
}

impl ApiError {
    /// An axum body rejection: over the body limit is a 413, anything else an unreadable body.
    pub fn from_body(rejection: BytesRejection) -> ApiError {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::TooLarge(format!(
                "the request body is larger than {} MiB",
                crate::BODY_LIMIT >> 20
            ))
        } else {
            ApiError::BadBody(rejection.body_text())
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::UnknownModel(_) | ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            ApiError::Busy(_) => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::MissingKey => StatusCode::FORBIDDEN,
            ApiError::WrongKey => StatusCode::UNAUTHORIZED,
            ApiError::BadBody(_) => StatusCode::BAD_REQUEST,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The JSON body.
    pub fn body(&self) -> Value {
        let detail = |error_type: &str, message: &str| json!({"detail": {"error_type": error_type, "message": message}});
        match self {
            ApiError::Validation(items) => json!({ "detail": items }),
            ApiError::TooLarge(msg) => detail("request_too_large", msg),
            ApiError::UnknownModel(msg) => detail("not_found_error", msg),
            ApiError::NotFound => json!({"detail": "Not Found"}),
            ApiError::MethodNotAllowed => json!({"detail": "Method Not Allowed"}),
            ApiError::Busy(msg) => detail("overloaded_error", msg),
            ApiError::MissingKey => detail(
                "authentication_error",
                "no API key; send it as `Authorization: Bearer <key>`",
            ),
            ApiError::WrongKey => detail("authentication_error", "invalid API key"),
            ApiError::BadBody(msg) => detail("invalid_request_error", msg),
            ApiError::Internal(msg) => detail("api_error", msg),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(self.body());
        match self {
            ApiError::Busy(_) => {
                (status, [(header::RETRY_AFTER, RETRY_AFTER_S)], body).into_response()
            }
            _ => (status, body).into_response(),
        }
    }
}
