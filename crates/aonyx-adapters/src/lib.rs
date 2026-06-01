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
}

/// Common surface every adapter implements.
#[async_trait]
pub trait ConversationAdapter: Send + Sync {
    /// Stable adapter name (`"telegram"`, `"discord"`, …).
    fn name(&self) -> &str;

    /// Run the adapter until cancellation (Ctrl-C).
    async fn run(&self) -> Result<()>;
}
