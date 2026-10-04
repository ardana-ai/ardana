//! [`ApiError`]: the only place HTTP error statuses and bodies are built, from `ardana-api`'s error bodies.
//!
//! Validation failures use FastAPI's shape, `{"detail": [{"loc", "msg", "type"}]}` with status 422, as decider and
//! TypeSafe answer them; every other error is `{"detail": {"error_type", "message"}}`, TypeSafe's live shape. Only a
//! failing runtime or model file is a 5xx: bad input never is. A request the model reader refuses is answered as
//! `ardana-core` words it ([`DecideError::status`], [`DecideError::body`]), which the playground's in-tab engine
//! answers too.

use ardana_api::ErrorBody;
use ardana_core::DecideError;
use axum::Json;
use axum::extract::rejection::BytesRejection;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

pub use ardana_api::ValidationItem;

/// Seconds a client waits before retrying a 503.
const RETRY_AFTER_S: &str = "1";

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// 422: the body is not a valid request.
    Validation(Vec<ValidationItem>),
    /// 422, 413 or 500: the model reader refused the request or failed, with `ardana-core`'s status and body.
    Decide { status: StatusCode, body: ErrorBody },
    /// 413: the body, the rows or the tokens exceed a limit, or a row exceeds the context window.
    TooLarge(String),
    /// 404: the request names a model the registry does not have; the message lists the available names.
    UnknownModel(String),
    /// 404 for a path the API does not have.
    NotFound,
    /// 405 for a method a path does not take.
    MethodNotAllowed,
    /// 416 for a byte range that starts past the end of a file of this many bytes.
    RangeNotSatisfiable(u64),
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
            ApiError::Decide { status, .. } => *status,
            ApiError::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::UnknownModel(_) | ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            ApiError::RangeNotSatisfiable(_) => StatusCode::RANGE_NOT_SATISFIABLE,
            ApiError::Busy(_) => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::MissingKey => StatusCode::FORBIDDEN,
            ApiError::WrongKey => StatusCode::UNAUTHORIZED,
            ApiError::BadBody(_) => StatusCode::BAD_REQUEST,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The JSON body.
    pub fn body(&self) -> ErrorBody {
        match self {
            ApiError::Validation(items) => ErrorBody::validation(items.clone()),
            ApiError::Decide { body, .. } => body.clone(),
            ApiError::TooLarge(msg) => ErrorBody::error("request_too_large", msg.as_str()),
            ApiError::UnknownModel(msg) => ErrorBody::error("not_found_error", msg.as_str()),
            ApiError::NotFound => ErrorBody::text("Not Found"),
            ApiError::MethodNotAllowed => ErrorBody::text("Method Not Allowed"),
            ApiError::RangeNotSatisfiable(_) => ErrorBody::text("Range Not Satisfiable"),
            ApiError::Busy(msg) => ErrorBody::error("overloaded_error", msg.as_str()),
            ApiError::MissingKey => ErrorBody::error(
                "authentication_error",
                "no API key; send it as `Authorization: Bearer <key>`",
            ),
            ApiError::WrongKey => ErrorBody::error("authentication_error", "invalid API key"),
            ApiError::BadBody(msg) => ErrorBody::error("invalid_request_error", msg.as_str()),
            ApiError::Internal(msg) => ErrorBody::error("api_error", msg.as_str()),
        }
    }
}

/// A decide failure, answered as `ardana-core` words it: bad input is 422, too much input 413, a runtime failure 500.
impl From<DecideError> for ApiError {
    fn from(err: DecideError) -> ApiError {
        ApiError::Decide {
            status: StatusCode::from_u16(err.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            body: err.body(),
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
            // The length the range missed, as RFC 9110 has a 416 name it.
            ApiError::RangeNotSatisfiable(len) => (
                status,
                [(header::CONTENT_RANGE, format!("bytes */{len}"))],
                body,
            )
                .into_response(),
            _ => (status, body).into_response(),
        }
    }
}
