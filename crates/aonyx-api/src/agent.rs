//! The injected agent-turn runner + the streamed frame type.
//!
//! The API owns session persistence; this trait owns only the loop: "given
//! the full message history, run one turn and return the post-turn log".
//! Keeping it a trait means `aonyx-api` never depends on `aonyx-agent`
//! (no dependency cycle), and the HTTP layer stays unit-testable with a
//! stub agent. The binary implements it over its real `AgentRunner`.

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

/// One agent turn over a full message history — blocking or streaming.
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
