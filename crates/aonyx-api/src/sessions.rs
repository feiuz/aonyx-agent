//! Session CRUD and the blocking-turn endpoint.
//!
//! Sessions are persisted in the [`SessionStore`](aonyx_memory::SessionStore)
//! so they survive restarts and are shared with the CLI/TUI. A turn loads the
//! stored history, appends the user message, runs the injected
//! [`ApiAgent`](crate::ApiAgent), and writes the new log back.

use aonyx_core::{Message, Role};
use aonyx_memory::SessionRecord;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::last_assistant_text;
use crate::error::{ApiError, ApiResult};
use crate::state::ApiState;

/// Query string for `GET /v1/sessions`.
#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// Project slug to scope the listing; defaults to the server project.
    pub project: Option<String>,
    /// Maximum rows to return (default 50).
    pub limit: Option<usize>,
}

/// A session list row — metadata only, no message bodies (cheap to list).
#[derive(Debug, Serialize)]
pub struct SessionSummary {
    /// Stable session id.
    pub id: Uuid,
    /// Project slug.
    pub project: String,
    /// One-line title derived from the first user message.
    pub title: String,
    /// Completed turns.
    pub turns: u32,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last-update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Parent session if forked.
    pub parent_id: Option<Uuid>,
}

impl From<SessionRecord> for SessionSummary {
    fn from(r: SessionRecord) -> Self {
        Self {
            id: r.id,
            project: r.project,
            title: r.title,
            turns: r.turns,
            created_at: r.created_at,
            updated_at: r.updated_at,
            parent_id: r.parent_id,
        }
    }
}

/// Body for `POST /v1/sessions`.
#[derive(Debug, Deserialize)]
pub struct NewSessionRequest {
    /// Project to scope the session; defaults to the server project.
    pub project: Option<String>,
    /// Optional system prompt seeded as the first message.
    pub system_prompt: Option<String>,
}

/// Body for `POST /v1/sessions/{id}/messages`.
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    /// The user message text.
    pub content: String,
}

/// Response of a blocking turn: the assistant reply plus the canonical,
/// post-turn session record (with the full message log).
#[derive(Debug, Serialize)]
pub struct TurnResponse {
    /// The assistant's reply text (last non-empty assistant message).
    pub reply: String,
    /// The stored session after the turn.
    pub session: SessionRecord,
}

/// `GET /v1/sessions` — list recent sessions for a project.
pub async fn list_sessions(
    State(state): State<ApiState>,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<Vec<SessionSummary>>> {
    let project = state.project_or_default(params.project);
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let rows = state.sessions.list_by_project(&project, limit).await?;
    Ok(Json(rows.into_iter().map(SessionSummary::from).collect()))
}

/// `POST /v1/sessions` — create a session (optionally seeded with a system
/// prompt).
pub async fn create_session(
    State(state): State<ApiState>,
    Json(req): Json<NewSessionRequest>,
) -> ApiResult<(StatusCode, Json<SessionRecord>)> {
    let project = state.project_or_default(req.project);
    let mut seed = Vec::new();
    if let Some(prompt) = req.system_prompt.filter(|p| !p.trim().is_empty()) {
        seed.push(Message::new(Role::System, prompt));
    }
    let record = state.sessions.create(&project, seed).await?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// `GET /v1/sessions/{id}` — fetch the full record including messages.
pub async fn get_session(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SessionRecord>> {
    let record = state
        .sessions
        .get(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no session {id}")))?;
    Ok(Json(record))
}

/// `DELETE /v1/sessions/{id}` — remove a session.
pub async fn delete_session(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.sessions.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/sessions/{id}/messages` — run one blocking turn.
pub async fn send_message(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> ApiResult<Json<TurnResponse>> {
    if req.content.trim().is_empty() {
        return Err(ApiError::BadRequest("empty message content".into()));
    }
    let record = state
        .sessions
        .get(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no session {id}")))?;

    let mut history = record.messages;
    history.push(Message::new(Role::User, req.content));

    let new_log = state.agent.run_turn(history).await?;
    let reply = last_assistant_text(&new_log);
    state.sessions.update(id, new_log, record.turns + 1).await?;

    // Re-read so the response carries the canonical stored state
    // (refreshed `updated_at` / `title`).
    let session = state
        .sessions
        .get(id)
        .await?
        .ok_or_else(|| ApiError::Internal("session vanished after update".into()))?;
    Ok(Json(TurnResponse { reply, session }))
}
