//! OpenAI-compatible chat endpoint, co-mounted on the same port so any
//! OpenAI SDK can drive the agent. Stateless: the client owns the history,
//! so each request carries the full `messages` array and we run one turn
//! over exactly those.

use aonyx_core::{Message, Role};
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::last_assistant_text;
use crate::error::{ApiError, ApiResult};
use crate::state::ApiState;

/// `POST /v1/chat/completions` request (the subset we honour).
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    /// Requested model (echoed back; the server uses its configured model).
    #[serde(default)]
    model: Option<String>,
    /// The conversation so far.
    messages: Vec<InMessage>,
}

#[derive(Debug, Deserialize)]
struct InMessage {
    role: String,
    #[serde(default)]
    content: String,
}

fn role_from_str(role: &str) -> Role {
    match role {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

/// `POST /v1/chat/completions` — OpenAI-compatible single completion.
pub async fn chat_completions(
    State(state): State<ApiState>,
    Json(req): Json<ChatRequest>,
) -> ApiResult<Json<Value>> {
    if req.messages.is_empty() {
        return Err(ApiError::BadRequest("`messages` must not be empty".into()));
    }
    let model = req.model.unwrap_or_else(|| state.info.model.clone());
    let history: Vec<Message> = req
        .messages
        .into_iter()
        .map(|m| Message::new(role_from_str(&m.role), m.content))
        .collect();

    let log = state.agent.run_turn(history).await?;
    let reply = last_assistant_text(&log);

    Ok(Json(json!({
        "id": "chatcmpl-aonyx",
        "object": "chat.completion",
        "created": 0,
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": reply },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
    })))
}

/// `GET /v1/models` — advertise the single Aonyx model id.
pub async fn models(State(state): State<ApiState>) -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [{ "id": state.info.model, "object": "model", "owned_by": "aonyx" }]
    }))
}
