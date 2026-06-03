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

/// A non-text payload riding alongside a [`Message`]'s textual
/// [`Message::content`] — used for vision-capable providers (Phase S).
///
/// `#[serde(tag = "type")]` keeps the on-the-wire shape compatible with
/// the way Anthropic / OpenAI describe content blocks, and lets future
/// variants (audio, PDF, …) land without breaking existing rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Attachment {
    /// Inline image, base64-encoded.
    Image {
        /// MIME type (e.g. `image/png`, `image/jpeg`).
        media_type: String,
        /// Base64 payload of the image bytes.
        data: String,
    },
}

/// A single message in a conversation log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Stable identifier.
    pub id: Uuid,
    /// Speaker role.
    pub role: Role,
    /// Plain-text payload.
    pub content: String,
    /// Wall-clock timestamp when the message was created.
    pub ts: DateTime<Utc>,
    /// Multimodal attachments riding alongside `content`. Defaults to
    /// empty so existing persisted rows deserialise unchanged.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Tool calls this (assistant) message requested. Non-empty only on an
    /// assistant turn that asked to invoke tools. Defaults to empty so older
    /// persisted rows decode unchanged.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// For a `Role::Tool` result message, the id of the [`ToolCall`] it
    /// answers (links the result back to the request). `None` otherwise.
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Construct a new text-only message with a fresh id and current
    /// timestamp.
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content: content.into(),
            ts: Utc::now(),
            attachments: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Construct a message carrying both text and one or more
    /// [`Attachment`]s (Phase S).
    pub fn with_attachments(
        role: Role,
        content: impl Into<String>,
        attachments: Vec<Attachment>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content: content.into(),
            ts: Utc::now(),
            attachments,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Construct an assistant message that requested one or more tool calls.
    /// `content` is the assistant's text (often empty when the model emits
    /// only tool calls).
    pub fn assistant_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            content: content.into(),
            ts: Utc::now(),
            attachments: Vec::new(),
            tool_calls,
            tool_call_id: None,
        }
    }

    /// Construct a `Role::Tool` result message answering the call `call_id`.
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role: Role::Tool,
            content: content.into(),
            ts: Utc::now(),
            attachments: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_new_starts_with_no_attachments() {
        let m = Message::new(Role::User, "hi");
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn message_with_attachments_carries_the_image_payload() {
        let att = Attachment::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo".into(),
        };
        let m = Message::with_attachments(Role::User, "look at this", vec![att.clone()]);
        assert_eq!(m.attachments, vec![att]);
        assert_eq!(m.content, "look at this");
    }

    #[test]
    fn legacy_messages_without_attachments_still_deserialise() {
        // No `attachments` field at all — should default to empty.
        let raw = serde_json::json!({
            "id": Uuid::new_v4(),
            "role": "user",
            "content": "hello",
            "ts": Utc::now()
        });
        let m: Message = serde_json::from_value(raw).expect("decodes");
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn attachment_image_round_trips_via_serde() {
        let att = Attachment::Image {
            media_type: "image/jpeg".into(),
            data: "/9j/4AAQ".into(),
        };
        let json = serde_json::to_value(&att).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["media_type"], "image/jpeg");
        let back: Attachment = serde_json::from_value(json).unwrap();
        assert_eq!(back, att);
    }
}
