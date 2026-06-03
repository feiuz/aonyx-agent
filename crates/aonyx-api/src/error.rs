//! The API error type and its HTTP rendering.
//!
//! Every handler returns [`ApiResult`]; an [`ApiError`] is converted into a
//! JSON body `{ "error": { "type": …, "message": … } }` with the matching
//! HTTP status. [`aonyx_core::AonyxError`] maps in via [`From`] so handlers
//! can use `?` over core calls.

use aonyx_core::AonyxError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Convenience alias for fallible handler bodies.
pub type ApiResult<T> = std::result::Result<T, ApiError>;

/// An error surfaced by an API handler, carrying enough to pick an HTTP
/// status and a stable machine-readable `type` tag.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Missing or invalid bearer token (`401`).
    #[error("{0}")]
    Unauthorized(String),

    /// The action is understood but not permitted — e.g. a Destructive tool
    /// invoked while `allow_destructive` is off (`403`).
    #[error("{0}")]
    Forbidden(String),

    /// The request was malformed or referenced something missing (`400`).
    #[error("{0}")]
    BadRequest(String),

    /// A referenced resource does not exist (`404`).
    #[error("{0}")]
    NotFound(String),

    /// Anything else — an internal failure (`500`).
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    /// HTTP status + stable `type` tag for this error.
    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            ApiError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, ty) = self.parts();
        let body = Json(json!({ "error": { "type": ty, "message": self.to_string() } }));
        (status, body).into_response()
    }
}

impl From<AonyxError> for ApiError {
    fn from(e: AonyxError) -> Self {
        match e {
            AonyxError::Config(m) => ApiError::BadRequest(m),
            AonyxError::ApprovalRejected(m) => ApiError::Forbidden(m),
            other => ApiError::Internal(other.to_string()),
        }
    }
}
