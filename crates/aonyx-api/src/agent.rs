//! The injected agent-turn runner, the streamed frame type, and the
//! read-only metadata DTOs (tools / skills / config).
//!
//! The API owns session + memory persistence directly; everything that lives
//! in `aonyx-agent` (the loop, the tool registry, the loaded skills, the
//! config) is reached through this trait so `aonyx-api` never depends on
//! `aonyx-agent` (no dependency cycle) and stays unit-testable with a stub.

use aonyx_core::{Message, Result, Role};
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc::Sender;

/// A single streamed event of a turn, serialized to the client as a JSON
/// frame (`{"type": "...", ...}`). Mirrors the agent loop's internal
/// `TurnEvent`; the binary maps its events onto these.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamFrame {
    /// Incremental assistant text.
    Delta {
        /// The chunk of assistant text.
        text: String,
    },
    /// A tool call is starting.
    ToolStart {
        /// Tool name.
        name: String,
        /// JSON arguments.
        args: serde_json::Value,
        /// Safety class (`safe` / `caution` / `destructive`).
        class: String,
    },
    /// A tool call finished.
    ToolEnd {
        /// Tool name.
        name: String,
        /// Whether the call succeeded.
        ok: bool,
        /// One-line outcome summary.
        summary: String,
    },
    /// A tool call was rejected by the approval gate.
    ToolRejected {
        /// Tool name.
        name: String,
        /// Safety class that triggered the rejection.
        class: String,
    },
    /// A destructive call is paused awaiting the user's approval. Resolve it
    /// with `POST /v1/approvals/:id` (echo `id`).
    ApprovalRequest {
        /// Tool call id — echo it back to resolve the decision.
        id: String,
        /// Tool name.
        name: String,
        /// JSON arguments the model wants to run with.
        args: serde_json::Value,
        /// Safety class (`destructive` in V1).
        class: String,
    },
    /// A new loop iteration began.
    Iteration {
        /// 1-based iteration number.
        n: u32,
    },
    /// The turn completed. Emitted by the HTTP layer after it persists the
    /// result — not by the agent.
    Done {
        /// Final assistant reply.
        reply: String,
        /// The session's new turn count.
        turns: u32,
    },
    /// The turn failed.
    Error {
        /// Human-readable error.
        message: String,
    },
}

/// Metadata for one registered tool (`GET /v1/tools`).
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    /// Tool name (matches `ToolHandler::name`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Safety class (`safe` / `caution` / `destructive`).
    pub class: String,
    /// JSON Schema for the tool's arguments.
    pub schema: serde_json::Value,
}

/// Metadata for one loaded skill (`GET /v1/skills`).
#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    /// Skill id (the `SKILL.md` slug).
    pub id: String,
    /// Human-readable name.
    #[serde(default)]
    pub name: String,
    /// Short description.
    pub description: String,
    /// Catalogue category (`software-development`, `creative`, …), if any.
    #[serde(default)]
    pub category: Option<String>,
    /// Free-form tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Trigger labels (keywords / patterns / `always`).
    pub triggers: Vec<String>,
}

/// Metadata for one sub-agent the architect can delegate to (`GET /v1/agents`).
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    /// Agent id (the `AGENT.md` slug).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// When to use this agent.
    pub description: String,
    /// Catalogue category, if any.
    #[serde(default)]
    pub category: Option<String>,
    /// Free-form tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Tool whitelist (empty = inherits the parent registry).
    #[serde(default)]
    pub tools: Vec<String>,
    /// `true` for a built-in catalogue preset, `false` for a user agent.
    #[serde(default)]
    pub builtin: bool,
}

/// Non-secret server configuration (`GET /v1/config`). Never carries keys.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConfigInfo {
    /// Active provider id.
    pub provider: String,
    /// Active default model id.
    pub model: String,
    /// Loop iteration cap.
    pub max_iterations: usize,
    /// Whether skill auto-generation is on.
    pub skill_autogen: bool,
}

/// One agent turn over a full message history — blocking or streaming — plus
/// read-only metadata accessors the binary fills from its live components.
#[async_trait]
pub trait ApiAgent: Send + Sync + 'static {
    /// Run one turn over `history`, returning the complete message log after
    /// the turn — the assistant reply, plus any tool messages the loop
    /// appended along the way.
    async fn run_turn(&self, history: Vec<Message>) -> Result<Vec<Message>>;

    /// Streaming variant: emit [`StreamFrame`]s (deltas, tool activity) on
    /// `tx` as the loop runs, returning the full post-turn log. The default
    /// runs the blocking turn and emits the reply as a single `Delta`; the
    /// binary overrides it to stream tokens + tool events live.
    ///
    /// Implementations must NOT emit `Done`/`Error` — the HTTP layer sends
    /// those after it persists the result.
    async fn run_turn_streaming(
        &self,
        history: Vec<Message>,
        tx: Sender<StreamFrame>,
    ) -> Result<Vec<Message>> {
        let log = self.run_turn(history).await?;
        let reply = last_assistant_text(&log);
        let _ = tx.send(StreamFrame::Delta { text: reply }).await;
        Ok(log)
    }

    /// The registered tools. Default: none (overridden by the binary).
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    /// The loaded skills. Default: none (overridden by the binary).
    fn skills(&self) -> Vec<SkillInfo> {
        Vec::new()
    }

    /// The available sub-agents (catalogue + user). Default: none.
    fn agents(&self) -> Vec<AgentInfo> {
        Vec::new()
    }

    /// The non-secret config snapshot. Default: empty (overridden by the
    /// binary).
    fn config(&self) -> ConfigInfo {
        ConfigInfo::default()
    }
}

/// The last non-empty assistant message in a log, or a placeholder when the
/// turn produced none.
pub(crate) fn last_assistant_text(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::Assistant) && !m.content.trim().is_empty())
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "(no reply)".to_string())
}
