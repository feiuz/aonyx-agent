//! The injected agent-turn runner.
//!
//! The API owns session persistence; this trait owns only the loop: "given
//! the full message history, run one turn and return the post-turn log".
//! Keeping it a trait means `aonyx-api` never depends on `aonyx-agent`
//! (no dependency cycle), and the HTTP layer stays unit-testable with a
//! stub agent. The binary implements it over its real `AgentRunner`.

use aonyx_core::{Message, Result, Role};
use async_trait::async_trait;

/// One blocking agent turn over a full message history.
#[async_trait]
pub trait ApiAgent: Send + Sync + 'static {
    /// Run one turn over `history`, returning the complete message log after
    /// the turn — the assistant reply, plus any tool messages the loop
    /// appended along the way.
    async fn run_turn(&self, history: Vec<Message>) -> Result<Vec<Message>>;
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
