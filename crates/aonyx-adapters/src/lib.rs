//! # aonyx-adapters
//!
//! Channel adapters that bridge external conversation platforms to the agent loop.
//! Every adapter implements a common [`ConversationAdapter`] trait so the loop
//! does not need to know whether it is talking to a CLI, Telegram, Discord, or
//! an OpenAI-compatible HTTP client.
//!
//! ## Planned adapters (Vague 2)
//! - [`telegram`] — via `teloxide`.
//! - [`discord`] — via `serenity`.
//! - [`openai_server`] — `axum` HTTP server exposing `/v1/chat/completions`.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod discord;
pub mod openai_server;
pub mod telegram;

use aonyx_core::Result;
use async_trait::async_trait;

/// Common surface every adapter implements.
#[async_trait]
pub trait ConversationAdapter: Send + Sync {
    /// Stable name (`"telegram"`, `"discord"`, `"openai_server"`, …).
    fn name(&self) -> &str;

    /// Run the adapter until cancellation.
    async fn run(&self) -> Result<()>;
}
