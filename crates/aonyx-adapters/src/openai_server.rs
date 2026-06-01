//! OpenAI-compatible HTTP server (Phase VV) — an `axum` server exposing
//! `POST /v1/chat/completions` so any OpenAI SDK can talk to the local
//! Aonyx Agent.
//!
//! Stateless: the client owns the conversation, so each request carries
//! the full `messages` array and we run one agent turn over it via
//! [`crate::AgentHandler::complete`]. An optional bearer token guards the
//! endpoint; otherwise bind to localhost only.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aonyx_core::{AonyxError, Result as AonyxResult};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::AgentHandler;

/// The OpenAI-compatible HTTP server.
pub struct OpenAiServer {
    addr: String,
    token: Option<String>,
    handler: Arc<dyn AgentHandler>,
}

impl OpenAiServer {
    /// Build the server bound to `addr` (e.g. `127.0.0.1:8787`), with an
    /// optional bearer token and the agent handler.
    pub fn new(
        addr: impl Into<String>,
        token: Option<String>,
        handler: Arc<dyn AgentHandler>,
    ) -> Self {
        Self {
            addr: addr.into(),
            token,
            handler,
        }
    }

    /// Serve until cancellation.
    pub async fn run(&self) -> AonyxResult<()> {
        let state = AppState {
            token: self.token.clone(),
            handler: Arc::clone(&self.handler),
        };
        let app = Router::new()
            .route("/v1/chat/completions", post(chat_completions))
            .route("/v1/models", get(list_models))
            .route("/health", get(|| async { "ok" }))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(&self.addr)
            .await
            .map_err(|e| AonyxError::Adapter(format!("bind {}: {e}", self.addr)))?;
        tracing::info!("openai-server: listening on {}", self.addr);
        axum::serve(listener, app)
            .await
            .map_err(|e| AonyxError::Adapter(format!("serve: {e}")))?;
        Ok(())
    }
}

#[derive(Clone)]
struct AppState {
    token: Option<String>,
    handler: Arc<dyn AgentHandler>,
}

#[derive(Deserialize)]
struct ChatRequest {
    #[serde(default)]
    model: Option<String>,
    messages: Vec<InMessage>,
}

#[derive(Deserialize)]
struct InMessage {
    role: String,
    #[serde(default)]
    content: String,
}

#[derive(Serialize)]
struct ChatResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Serialize)]
struct Choice {
    index: u32,
    message: OutMessage,
    finish_reason: &'static str,
}

#[derive(Serialize)]
struct OutMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `true` when no token is configured, or the request carries the right
/// `Authorization: Bearer <token>`.
fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    match &state.token {
        None => true,
        Some(expected) => headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|h| h.strip_prefix("Bearer ").unwrap_or(h) == expected)
            .unwrap_or(false),
    }
}

async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    if !authorized(&state, &headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": { "message": "missing or invalid bearer token", "type": "auth" } })),
        )
            .into_response();
    }
    let model = req.model.unwrap_or_else(|| "aonyx".to_string());
    let messages: Vec<(String, String)> = req
        .messages
        .into_iter()
        .map(|m| (m.role, m.content))
        .collect();
    match state.handler.complete(messages).await {
        Ok(text) => {
            let resp = ChatResponse {
                id: format!("chatcmpl-{}", unix_now()),
                object: "chat.completion",
                created: unix_now(),
                model,
                choices: vec![Choice {
                    index: 0,
                    message: OutMessage {
                        role: "assistant",
                        content: text,
                    },
                    finish_reason: "stop",
                }],
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "message": e.to_string(), "type": "agent" } })),
        )
            .into_response(),
    }
}

async fn list_models() -> impl IntoResponse {
    Json(json!({
        "object": "list",
        "data": [ { "id": "aonyx", "object": "model", "owned_by": "aonyx" } ]
    }))
}
