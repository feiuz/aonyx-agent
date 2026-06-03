//! HTTP routes and the public router builder.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::auth::require_auth;
use crate::state::{ApiState, ServerInfo};

/// Build the complete API router for the given [`ApiState`].
///
/// `/v1/health` is public; every other route sits behind the bearer-token
/// middleware (a no-op when no token is configured). Later V4 phases merge
/// more route groups here (sessions, memory, tools, skills, config).
pub fn build_router(state: ApiState) -> Router {
    let public = Router::new().route("/v1/health", get(health));

    let protected = Router::new()
        .route("/v1/info", get(info))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    public.merge(protected).with_state(state)
}

/// Liveness probe — always open, no auth.
async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// Server identity + capabilities (auth required).
async fn info(State(state): State<ApiState>) -> Json<ServerInfo> {
    Json((*state.info).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AuthConfig, ServerInfo};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // for `oneshot`

    fn state(token: Option<&str>) -> ApiState {
        ApiState::new(
            AuthConfig::new(token.map(str::to_string), false),
            ServerInfo::new("anthropic", "claude-test", vec!["api".into()]),
        )
    }

    fn req(uri: &str, bearer: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().uri(uri);
        if let Some(t) = bearer {
            b = b.header(axum::http::header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn health_is_open_even_with_a_token_set() {
        let app = build_router(state(Some("secret")));
        let res = app.oneshot(req("/v1/health", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn info_rejects_without_token() {
        let app = build_router(state(Some("secret")));
        let res = app.oneshot(req("/v1/info", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn info_rejects_wrong_token() {
        let app = build_router(state(Some("secret")));
        let res = app.oneshot(req("/v1/info", Some("nope"))).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn info_accepts_right_token() {
        let app = build_router(state(Some("secret")));
        let res = app.oneshot(req("/v1/info", Some("secret"))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn info_open_when_no_token_configured() {
        let app = build_router(state(None));
        let res = app.oneshot(req("/v1/info", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
