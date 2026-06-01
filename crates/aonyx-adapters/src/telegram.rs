//! Telegram adapter (Phase TT) — a `teloxide` long-poll bot bridged to
//! the agent loop via [`crate::AgentHandler`].
//!
//! Each Telegram chat maps to one conversation (`chat_id`); the handler
//! keeps per-chat history. Long replies are split to respect Telegram's
//! 4096-char message cap.

use std::sync::Arc;

use aonyx_core::Result as AonyxResult;
use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::ChatAction;

use crate::{AgentHandler, ConversationAdapter};

/// A Telegram bot adapter.
pub struct TelegramAdapter {
    token: String,
    /// Allowed chat ids; empty = accept every chat.
    allowed: Vec<i64>,
    handler: Arc<dyn AgentHandler>,
}

impl TelegramAdapter {
    /// Build the adapter from a bot token, an allow-list of chat ids
    /// (empty = all), and the agent handler the binary supplies.
    pub fn new(
        token: impl Into<String>,
        allowed: Vec<i64>,
        handler: Arc<dyn AgentHandler>,
    ) -> Self {
        Self {
            token: token.into(),
            allowed,
            handler,
        }
    }
}

/// Telegram hard-caps a message at 4096 chars; split a long reply on line
/// boundaries (hard-splitting any single over-long line).
fn chunk_message(s: &str) -> Vec<String> {
    const LIMIT: usize = 4000;
    if s.chars().count() <= LIMIT {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut buf = String::new();
    for line in s.split_inclusive('\n') {
        if !buf.is_empty() && buf.chars().count() + line.chars().count() > LIMIT {
            out.push(std::mem::take(&mut buf));
        }
        if line.chars().count() > LIMIT {
            for ch in line.chars() {
                buf.push(ch);
                if buf.chars().count() >= LIMIT {
                    out.push(std::mem::take(&mut buf));
                }
            }
        } else {
            buf.push_str(line);
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

#[async_trait]
impl ConversationAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn run(&self) -> AonyxResult<()> {
        let bot = Bot::new(&self.token);
        let handler = Arc::clone(&self.handler);
        let allowed = Arc::new(self.allowed.clone());

        tracing::info!("telegram: bot ready (long-poll); Ctrl-C to stop");
        teloxide::repl(bot, move |bot: Bot, msg: Message| {
            let handler = Arc::clone(&handler);
            let allowed = Arc::clone(&allowed);
            async move {
                let chat_id = msg.chat.id;
                // Allow-list gate (empty list = open to all chats).
                if !allowed.is_empty() && !allowed.contains(&chat_id.0) {
                    return Ok(());
                }
                let Some(text) = msg.text() else {
                    return Ok(());
                };
                // Best-effort "typing…" while the agent thinks.
                let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
                let reply = match handler.handle(&chat_id.0.to_string(), text).await {
                    Ok(r) if !r.trim().is_empty() => r,
                    Ok(_) => "(no reply)".to_string(),
                    Err(e) => format!("⚠ {e}"),
                };
                for part in chunk_message(&reply) {
                    let _ = bot.send_message(chat_id, part).await;
                }
                Ok(())
            }
        })
        .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::chunk_message;

    #[test]
    fn short_message_is_one_chunk() {
        assert_eq!(chunk_message("hello").len(), 1);
    }

    #[test]
    fn long_message_is_split_under_the_cap() {
        let big = "x\n".repeat(5000); // 10_000 chars
        let parts = chunk_message(&big);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|p| p.chars().count() <= 4000));
        assert_eq!(parts.concat(), big);
    }

    #[test]
    fn single_overlong_line_is_hard_split() {
        let big = "y".repeat(9000);
        let parts = chunk_message(&big);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.chars().count() <= 4000));
        assert_eq!(parts.concat(), big);
    }
}
