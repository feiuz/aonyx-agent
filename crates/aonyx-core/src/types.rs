//! Domain types shared across the workspace.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Conversation roles (OpenAI-style, with explicit Tool role).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System prompt or persona.
    System,
    /// End user.
    User,
    /// Model.
    Assistant,
    /// Tool result message.
    Tool,
}

/// A single message in a conversation log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Stable identifier.
    pub id: Uuid,
    /// Speaker role.
    pub role: Role,
    /// Plain-text payload. Multimodal extensions live elsewhere.
    pub content: String,
    /// Wall-clock timestamp when the message was created.
    pub ts: DateTime<Utc>,
}

impl Message {
    /// Construct a new message with a fresh id and current timestamp.
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content: content.into(),
            ts: Utc::now(),
        }
    }
}

/// A request from the model to invoke a registered tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-issued call id.
    pub id: String,
    /// Registered tool name (must match `ToolHandler::name`).
    pub name: String,
    /// JSON arguments — validated against the tool's schema before invocation.
    pub args: serde_json::Value,
}

/// The outcome of a single tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Pairs with `ToolCall::id`.
    pub call_id: String,
    /// Successful output (may be `null`).
    pub output: serde_json::Value,
    /// Error message if the tool failed.
    pub error: Option<String>,
}

/// Per-tool safety classification used by the approval gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SafetyClass {
    /// Read-only or otherwise risk-free.
    Safe,
    /// Mutates local state in a reversible way.
    Caution,
    /// Irreversible or externally visible (delete, push, send).
    Destructive,
}

/// A logical agent session — a thread of conversation tied to (optionally) a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Stable identifier.
    pub id: Uuid,
    /// Project slug (e.g. `./my-research`) if the session is scoped.
    pub project: Option<String>,
    /// Session creation time.
    pub created_at: DateTime<Utc>,
    /// Parent session for fork / compaction lineage.
    pub parent_session_id: Option<Uuid>,
}

impl Session {
    /// Create a new top-level session.
    pub fn new(project: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            project,
            created_at: Utc::now(),
            parent_session_id: None,
        }
    }
}
