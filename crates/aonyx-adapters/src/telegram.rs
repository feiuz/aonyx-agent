//! Telegram adapter (Phase TT) — a `teloxide` long-poll bot bridged to
//! the agent loop via [`crate::AgentHandler`].
//!
//! Each Telegram chat maps to one conversation (`chat_id`); the handler
//! keeps per-chat history. Long replies are split to respect Telegram's
//! 4096-char message cap.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aonyx_core::Result as AonyxResult;
use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ChatId, MessageId};
use tokio::sync::mpsc;

use crate::{AgentHandler, ConversationAdapter, StreamEvent};

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
                // Stream the turn into a single message, edited live.
                stream_reply(&bot, chat_id, &handler, text).await;
                Ok(())
            }
        })
        .await;
        Ok(())
    }
}

/// Stream one agent turn into a single Telegram message: send a placeholder,
/// edit it in place as [`StreamEvent`]s arrive (throttled to dodge the
/// ~1 edit/s per-chat rate limit), then finalise with the complete reply —
/// chunked across messages if it exceeds Telegram's 4096-char cap.
async fn stream_reply(bot: &Bot, chat_id: ChatId, handler: &Arc<dyn AgentHandler>, text: &str) {
    // "typing…" then a placeholder we keep editing as tokens arrive.
    let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
    let sent = match bot.send_message(chat_id, "…").await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("telegram: placeholder send failed: {e}");
            return;
        }
    };
    let msg_id = sent.id;

    // Spawn the agent turn; it streams StreamEvents back on `tx`.
    let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);
    {
        let handler = Arc::clone(handler);
        let chat_key = chat_id.0.to_string();
        let user_text = text.to_string();
        tokio::spawn(async move {
            if let Err(e) = handler.handle_stream(&chat_key, &user_text, tx).await {
                tracing::warn!("telegram: handle_stream: {e}");
            }
        });
    }

    let throttle = Duration::from_millis(900);
    let mut buf = String::new();
    let mut shown = String::from("…"); // what the message currently displays
    let mut last_edit = Instant::now();
    let mut painted = false;

    while let Some(ev) = rx.recv().await {
        match ev {
            // Transient status (tool activity) — appended below the live text.
            StreamEvent::Status(s) => {
                let view = if buf.is_empty() {
                    s
                } else {
                    format!("{}\n\n{}", stream_window(&buf), s)
                };
                edit_if_changed(bot, chat_id, msg_id, &view, &mut shown).await;
                last_edit = Instant::now();
                painted = true;
            }
            // Incremental tokens — paint first one immediately, then throttle.
            StreamEvent::Delta(d) => {
                buf.push_str(&d);
                if (!painted || last_edit.elapsed() >= throttle) && !buf.trim().is_empty() {
                    let view = stream_window(&buf);
                    edit_if_changed(bot, chat_id, msg_id, &view, &mut shown).await;
                    last_edit = Instant::now();
                    painted = true;
                }
            }
            // Authoritative final reply — first chunk replaces the streamed
            // message; any overflow chunks go out as new messages.
            StreamEvent::Final(f) => {
                let f = if f.trim().is_empty() {
                    "(no reply)".to_string()
                } else {
                    f
                };
                let parts = chunk_message(&f);
                if let Some(first) = parts.first() {
                    edit_if_changed(bot, chat_id, msg_id, first, &mut shown).await;
                }
                for extra in parts.iter().skip(1) {
                    let _ = bot.send_message(chat_id, extra.clone()).await;
                }
            }
        }
    }
}

/// Keep a live (pre-final) edit under Telegram's 4096-char cap by showing the
/// tail of the buffer (the newest text) behind a leading ellipsis.
fn stream_window(s: &str) -> String {
    const CAP: usize = 3900;
    let n = s.chars().count();
    if n <= CAP {
        s.to_string()
    } else {
        let tail: String = s.chars().skip(n - CAP).collect();
        format!("…{tail}")
    }
}

/// Edit the message only when the text actually changed — Telegram answers
/// 400 "message is not modified" otherwise. Records the new text on success.
async fn edit_if_changed(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    text: &str,
    shown: &mut String,
) {
    if text == shown {
        return;
    }
    match bot.edit_message_text(chat_id, msg_id, text).await {
        Ok(_) => *shown = text.to_string(),
        // Rate-limit (429) or transient error — keep going; the Final edit
        // reconciles the displayed text.
        Err(e) => tracing::debug!("telegram: edit skipped: {e}"),
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
