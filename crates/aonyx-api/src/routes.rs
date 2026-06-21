//! HTTP routes and the public router builder.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::auth::require_auth;
use crate::sessions::{create_session, delete_session, get_session, list_sessions, send_message};
use crate::state::{ApiState, ServerInfo};
use crate::streaming::{sse_message, ws_stream};
use crate::{memory, meta, openai};

/// Build the complete API router for the given [`ApiState`].
///
/// `/v1/health` is public; every other route sits behind the bearer-token
/// middleware (a no-op when no token is configured). Later V4 phases merge
/// more route groups here (memory, tools, skills, config, streaming).
pub fn build_router(state: ApiState) -> Router {
    let public = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/openapi.json", get(meta::openapi));

    let protected = Router::new()
        .route("/v1/info", get(info))
        .route("/v1/sessions", get(list_sessions).post(create_session))
        .route("/v1/sessions/:id", get(get_session).delete(delete_session))
        .route("/v1/sessions/:id/messages", post(send_message))
        .route("/v1/sessions/:id/messages/stream", post(sse_message))
        .route("/v1/sessions/:id/stream", get(ws_stream))
        .route("/v1/approvals/:id", post(resolve_approval))
        .route("/v1/memory/search", get(memory::search))
        .route("/v1/memory/ingest", post(memory::ingest))
        .route("/v1/memory/projects", get(memory::projects))
        .route(
            "/v1/memory/diary",
            get(memory::diary_list).post(memory::diary_append),
        )
        .route("/v1/memory/kg/entities", get(memory::kg_entities))
        .route(
            "/v1/memory/kg/entities/:name",
            get(memory::kg_entity_by_name),
        )
        .route("/v1/memory/kg/relations", get(memory::kg_relations))
        .route("/v1/tools", get(meta::list_tools))
        .route("/v1/tools/:name/enabled", post(set_tool_enabled))
        .route("/v1/skills", get(meta::list_skills))
        .route("/v1/agents", get(meta::list_agents))
        .route("/v1/skills/:id/enabled", post(set_skill_enabled))
        .route("/v1/config", get(meta::get_config))
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/models", get(openai::models))
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

/// A client's decision on a paused interactive approval.
#[derive(Deserialize)]
struct ApprovalDecision {
    approved: bool,
}

/// Resolve a paused destructive tool call (auth required). `204` when the
/// pending approval was found and resolved, `404` when `id` is unknown (already
/// resolved, timed out, or never registered).
async fn resolve_approval(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(body): Json<ApprovalDecision>,
) -> StatusCode {
    if state.approvals.resolve(&id, body.approved) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// A client's request to enable or disable a tool.
#[derive(Deserialize)]
struct ToolToggle {
    enabled: bool,
}

/// Enable or disable a registered tool for the next turn (auth required). The
/// runner's registry shares this set, so the change is live — a disabled tool
/// stops being offered to the model.
async fn set_tool_enabled(
    State(state): State<ApiState>,
    Path(name): Path<String>,
    Json(body): Json<ToolToggle>,
) -> StatusCode {
    match state.tool_disabled.lock() {
        Ok(mut d) => {
            if body.enabled {
                d.remove(&name);
            } else {
                d.insert(name);
            }
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Enable or disable a skill for the next turn (auth required). The runner
/// shares this set, so the change is live — a disabled skill stops matching.
async fn set_skill_enabled(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(body): Json<ToolToggle>,
) -> StatusCode {
    match state.skill_disabled.lock() {
        Ok(mut d) => {
            if body.enabled {
                d.remove(&id);
            } else {
                d.insert(id);
            }
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ApiAgent;
    use crate::state::{AuthConfig, ServerInfo};
    use aonyx_core::{Message, Role};
    use aonyx_memory::{Palace, SqliteSessionStore};
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt; // for `oneshot`

    /// Stub agent: echoes the last user message back as an assistant reply.
    struct EchoAgent;

    #[async_trait]
    impl ApiAgent for EchoAgent {
        async fn run_turn(&self, mut history: Vec<Message>) -> aonyx_core::Result<Vec<Message>> {
            let last_user = history
                .iter()
                .rev()
                .find(|m| matches!(m.role, Role::User))
                .map(|m| m.content.clone())
                .unwrap_or_default();
            history.push(Message::new(Role::Assistant, format!("echo: {last_user}")));
            Ok(history)
        }
    }

    fn state(token: Option<&str>) -> ApiState {
        ApiState::new(
            AuthConfig::new(token.map(str::to_string), false),
            ServerInfo::new("anthropic", "claude-test", vec!["api".into()]),
            Arc::new(SqliteSessionStore::open_in_memory().unwrap()),
            Arc::new(Palace::open_in_memory().unwrap()),
            Arc::new(EchoAgent),
            "demo",
        )
    }

    fn get_req(uri: &str, bearer: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().uri(uri);
        if let Some(t) = bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    fn post_req(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn delete_req(uri: &str) -> Request<Body> {
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    }

    async fn body_json(res: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ---- auth ----------------------------------------------------------

    #[tokio::test]
    async fn health_is_open_even_with_a_token_set() {
        let app = build_router(state(Some("secret")));
        let res = app.oneshot(get_req("/v1/health", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn info_rejects_without_token() {
        let app = build_router(state(Some("secret")));
        let res = app.oneshot(get_req("/v1/info", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn info_rejects_wrong_token() {
        let app = build_router(state(Some("secret")));
        let res = app
            .oneshot(get_req("/v1/info", Some("nope")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn info_accepts_right_token() {
        let app = build_router(state(Some("secret")));
        let res = app
            .oneshot(get_req("/v1/info", Some("secret")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn info_open_when_no_token_configured() {
        let app = build_router(state(None));
        let res = app.oneshot(get_req("/v1/info", None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    // ---- sessions + turns ---------------------------------------------

    #[tokio::test]
    async fn create_then_send_then_get_round_trips() {
        let app = build_router(state(None));

        // create
        let res = app
            .clone()
            .oneshot(post_req("/v1/sessions", json!({ "project": "demo" })))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let created = body_json(res).await;
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["turns"], 0);

        // send a turn
        let res = app
            .clone()
            .oneshot(post_req(
                &format!("/v1/sessions/{id}/messages"),
                json!({ "content": "hello" }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let turn = body_json(res).await;
        assert_eq!(turn["reply"], "echo: hello");
        assert_eq!(turn["session"]["turns"], 1);

        // get reflects the persisted history (user + assistant)
        let res = app
            .clone()
            .oneshot(get_req(&format!("/v1/sessions/{id}"), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let got = body_json(res).await;
        assert_eq!(got["messages"].as_array().unwrap().len(), 2);
        assert_eq!(got["title"], "hello");
    }

    #[tokio::test]
    async fn list_shows_created_session() {
        let app = build_router(state(None));
        app.clone()
            .oneshot(post_req("/v1/sessions", json!({ "project": "demo" })))
            .await
            .unwrap();
        let res = app
            .oneshot(get_req("/v1/sessions?project=demo", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let list = body_json(res).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_missing_session_is_404() {
        let app = build_router(state(None));
        let res = app
            .oneshot(get_req(
                "/v1/sessions/00000000-0000-0000-0000-000000000000",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_removes_session() {
        let app = build_router(state(None));
        let res = app
            .clone()
            .oneshot(post_req("/v1/sessions", json!({})))
            .await
            .unwrap();
        let id = body_json(res).await["id"].as_str().unwrap().to_string();

        let res = app
            .clone()
            .oneshot(delete_req(&format!("/v1/sessions/{id}")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let res = app
            .oneshot(get_req(&format!("/v1/sessions/{id}"), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn empty_message_is_400() {
        let app = build_router(state(None));
        let res = app
            .clone()
            .oneshot(post_req("/v1/sessions", json!({})))
            .await
            .unwrap();
        let id = body_json(res).await["id"].as_str().unwrap().to_string();
        let res = app
            .oneshot(post_req(
                &format!("/v1/sessions/{id}/messages"),
                json!({ "content": "   " }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn sessions_require_auth_when_token_set() {
        let app = build_router(state(Some("secret")));
        let res = app
            .oneshot(post_req("/v1/sessions", json!({})))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // ---- streaming (SSE + WS) -----------------------------------------

    #[tokio::test]
    async fn sse_streams_delta_then_done_and_persists() {
        let app = build_router(state(None));
        let res = app
            .clone()
            .oneshot(post_req("/v1/sessions", json!({})))
            .await
            .unwrap();
        let id = body_json(res).await["id"].as_str().unwrap().to_string();

        let res = app
            .clone()
            .oneshot(post_req(
                &format!("/v1/sessions/{id}/messages/stream"),
                json!({ "content": "hi" }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        assert!(ct.starts_with("text/event-stream"), "content-type: {ct}");

        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("\"type\":\"delta\""), "body: {text}");
        assert!(text.contains("echo: hi"), "body: {text}");
        assert!(text.contains("\"type\":\"done\""), "body: {text}");

        // the turn was persisted (user + assistant)
        let got = body_json(
            app.oneshot(get_req(&format!("/v1/sessions/{id}"), None))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(got["messages"].as_array().unwrap().len(), 2);
        assert_eq!(got["turns"], 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ws_streams_a_turn_and_persists() {
        use aonyx_memory::SessionStore;
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let store = Arc::new(SqliteSessionStore::open_in_memory().unwrap());
        let created = store.create("demo", Vec::new()).await.unwrap();

        let st = ApiState::new(
            AuthConfig::new(None, false),
            ServerInfo::new("anthropic", "claude-test", vec!["api".into()]),
            store.clone(),
            Arc::new(Palace::open_in_memory().unwrap()),
            Arc::new(EchoAgent),
            "demo",
        );
        let app = build_router(st);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{addr}/v1/sessions/{}/stream", created.id);
        let (mut ws, _resp) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .unwrap();
        ws.send(TMsg::Text(
            json!({ "type": "user", "content": "hi" }).to_string(),
        ))
        .await
        .unwrap();

        let mut saw_delta = false;
        let mut saw_done = false;
        while let Some(Ok(msg)) = ws.next().await {
            if let TMsg::Text(t) = msg {
                if t.contains("\"type\":\"delta\"") {
                    saw_delta = true;
                }
                if t.contains("\"type\":\"done\"") {
                    saw_done = true;
                    break;
                }
            }
        }
        assert!(saw_delta, "expected a delta frame");
        assert!(saw_done, "expected a done frame");

        let got = store.get(created.id).await.unwrap().unwrap();
        assert_eq!(got.messages.len(), 2);
        assert_eq!(got.turns, 1);
    }

    // ---- memory + metadata --------------------------------------------

    #[tokio::test]
    async fn diary_append_then_list() {
        let app = build_router(state(None));
        let res = app
            .clone()
            .oneshot(post_req("/v1/memory/diary", json!({ "content": "a note" })))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app
            .oneshot(get_req("/v1/memory/diary?project=demo", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let entries = body_json(res).await;
        assert_eq!(entries.as_array().unwrap().len(), 1);
        assert_eq!(entries[0]["content"], "a note");
    }

    #[tokio::test]
    async fn kg_entities_empty_is_ok() {
        let app = build_router(state(None));
        let res = app
            .oneshot(get_req("/v1/memory/kg/entities", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn search_without_query_is_400() {
        let app = build_router(state(None));
        let res = app
            .oneshot(get_req("/v1/memory/search", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn tools_skills_config_default_ok() {
        let app = build_router(state(None));
        for path in ["/v1/tools", "/v1/skills", "/v1/config"] {
            let res = app.clone().oneshot(get_req(path, None)).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK, "for {path}");
        }
    }

    #[tokio::test]
    async fn openapi_is_public_and_well_formed() {
        // token set, but the spec route is public
        let app = build_router(state(Some("secret")));
        let res = app
            .oneshot(get_req("/v1/openapi.json", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let spec = body_json(res).await;
        assert_eq!(spec["openapi"], "3.0.3");
        assert!(spec["paths"]["/v1/sessions"].is_object());
        assert!(spec["paths"]["/v1/memory/search"].is_object());
    }

    #[tokio::test]
    async fn openai_chat_completions_runs_a_turn() {
        let app = build_router(state(None));
        let res = app
            .oneshot(post_req(
                "/v1/chat/completions",
                json!({ "messages": [{ "role": "user", "content": "hello" }] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(body["choices"][0]["message"]["content"], "echo: hello");
    }
}
