//! Bearer-token authentication middleware.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::error::ApiError;
use crate::state::ApiState;

/// Axum middleware enforcing the configured bearer token. Applied to the
/// protected route group only, so `/v1/health` stays open. When no token is
/// configured ([`AuthConfig::check`](crate::AuthConfig::check) returns
/// `true`), requests pass through — intended for loopback-only deployments.
pub async fn require_auth(
    State(state): State<ApiState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if state.auth.check(header) {
        Ok(next.run(req).await)
    } else {
        Err(ApiError::Unauthorized(
            "missing or invalid bearer token".into(),
        ))
    }
}
