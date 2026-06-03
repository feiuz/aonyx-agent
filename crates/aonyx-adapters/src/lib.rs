//! # aonyx-adapters
//!
//! Channel adapters that bridge external conversation platforms to the
//! agent loop. Each adapter is decoupled from the agent crate: it calls
//! an [`AgentHandler`] the binary supplies, so this crate depends only on
//! `aonyx-core` and never forms a dependency cycle with `aonyx-agent`.
//!
//! Heavy platform SDKs are opt-in via cargo features so the default
//! `aonyx` binary stays lean:
//! - `telegram` — [`telegram`] via `teloxide`.
//! - `discord` — via `serenity` (Phase UU).
//! - `openai-server` — an `axum` HTTP server (Phase VV).

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

#[cfg(feature = "discord")]
pub mod discord;
#[cfg(feature = "openai-server")]
pub mod openai_server;
#[cfg(feature = "telegram")]
pub mod telegram;

use aonyx_core::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Progressive output of a streaming agent turn, surfaced to adapters that
/// can render incrementally (e.g. Telegram's `editMessageText`).
///
/// The producer (the binary's handler) stays decoupled from the agent crate:
/// it translates the runner's internal events into these three cases, so the
/// adapter never depends on `aonyx-agent`.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A transient status line (tool activity, "thinking…") — shown in place,
    /// not appended to the reply buffer.
    Status(String),
    /// Incremental assistant text — append to the running reply.
    Delta(String),
    /// The complete, final reply. Adapters apply rich formatting / chunking
    /// here. Emitted exactly once at the end (even on error, carrying the
    /// error text) so an adapter can rely on it to finalise its message.
    Final(String),
}

/// What an adapter invokes for each inbound message. The binary
/// implements this over an `AgentRunner`; the adapter stays agnostic of
/// the agent internals.
///
/// `chat_id` is a stable per-conversation key (a Telegram chat id, a
/// Discord channel id, …) so the handler can keep separate history per
/// conversation.
#[async_trait]
pub trait AgentHandler: Send + Sync + 'static {
    /// Produce the agent's reply to `text` arriving on `chat_id`.
    async fn handle(&self, chat_id: &str, text: &str) -> Result<String>;

    /// Run one **stateless** turn over a full `(role, content)` message
    /// list — used by the OpenAI-compatible server, where the client owns
    /// the conversation history. Defaults to unsupported so chat adapters
    /// need not implement it.
    async fn complete(&self, _messages: Vec<(String, String)>) -> Result<String> {
        Err(aonyx_core::AonyxError::Adapter(
            "this handler does not support stateless completion".into(),
        ))
    }

    /// Stream the reply to `text` on `chat_id` as [`StreamEvent`]s on `tx`.
    ///
    /// The default is back-compatible: it runs the blocking
    /// [`AgentHandler::handle`] and emits the whole reply as a single
    /// [`StreamEvent::Final`]. Adapters that can't stream (Discord, …) keep
    /// working unchanged; a handler that can stream overrides this to emit
    /// [`StreamEvent::Delta`] / [`StreamEvent::Status`] as the turn unfolds,
    /// always ending with exactly one [`StreamEvent::Final`].
    async fn handle_stream(
        &self,
        chat_id: &str,
        text: &str,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        let reply = self.handle(chat_id, text).await?;
        let _ = tx.send(StreamEvent::Final(reply)).await;
        Ok(())
    }
}

/// Common surface every adapter implements.
#[async_trait]
pub trait ConversationAdapter: Send + Sync {
    /// Stable adapter name (`"telegram"`, `"discord"`, …).
    fn name(&self) -> &str;

    /// Run the adapter until cancellation (Ctrl-C).
    async fn run(&self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handler that only implements the blocking `handle` — it relies on the
    /// default `handle_stream`, exactly like the non-streaming adapters do.
    struct Echo;

    #[async_trait]
    impl AgentHandler for Echo {
        async fn handle(&self, _chat_id: &str, text: &str) -> Result<String> {
            Ok(format!("echo: {text}"))
        }
    }

    #[tokio::test]
    async fn default_handle_stream_emits_a_single_final() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent>(8);
        Echo.handle_stream("chat-1", "hi", tx).await.unwrap();

        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        assert_eq!(events.len(), 1, "default must emit exactly one event");
        match &events[0] {
            StreamEvent::Final(s) => assert_eq!(s, "echo: hi"),
            other => panic!("expected Final, got {other:?}"),
        }
    }
}
