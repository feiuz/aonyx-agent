//! Discord adapter (Phase UU) — a `serenity` gateway bot bridged to the
//! agent loop via [`crate::AgentHandler`].
//!
//! Each Discord channel maps to one conversation (`channel_id`). The bot
//! needs the **MESSAGE CONTENT** privileged intent enabled in the Discord
//! developer portal, otherwise inbound `content` is empty. Replies are
//! split to respect Discord's 2000-char message cap.

use std::sync::Arc;

use aonyx_core::{AonyxError, Result as AonyxResult};
use serenity::all::{Context, EventHandler, GatewayIntents, Message as DiscordMessage};
use serenity::Client;

use crate::{AgentHandler, ConversationAdapter};

/// A Discord gateway bot adapter.
pub struct DiscordAdapter {
    token: String,
    /// Allowed channel ids; empty = accept every channel.
    allowed: Vec<i64>,
    handler: Arc<dyn AgentHandler>,
}

impl DiscordAdapter {
    /// Build the adapter from a bot token, an allow-list of channel ids
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

/// serenity event handler that forwards each message to the agent.
struct Bridge {
    handler: Arc<dyn AgentHandler>,
    allowed: Vec<i64>,
}

#[serenity::async_trait]
impl EventHandler for Bridge {
    async fn message(&self, ctx: Context, msg: DiscordMessage) {
        if msg.author.bot {
            return;
        }
        let channel = msg.channel_id.get() as i64;
        if !self.allowed.is_empty() && !self.allowed.contains(&channel) {
            return;
        }
        let text = msg.content.trim().to_string();
        if text.is_empty() {
            return;
        }
        let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
        let reply = match self.handler.handle(&channel.to_string(), &text).await {
            Ok(r) if !r.trim().is_empty() => r,
            Ok(_) => "(no reply)".to_string(),
            Err(e) => format!("⚠ {e}"),
        };
        for part in chunk_message(&reply, 2000) {
            let _ = msg.channel_id.say(&ctx.http, part).await;
        }
    }
}

/// Split a reply into Discord-sized (`limit`) chunks on line boundaries,
/// hard-splitting any single over-long line.
fn chunk_message(s: &str, limit: usize) -> Vec<String> {
    if s.chars().count() <= limit {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut buf = String::new();
    for line in s.split_inclusive('\n') {
        if !buf.is_empty() && buf.chars().count() + line.chars().count() > limit {
            out.push(std::mem::take(&mut buf));
        }
        if line.chars().count() > limit {
            for ch in line.chars() {
                buf.push(ch);
                if buf.chars().count() >= limit {
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

#[async_trait::async_trait]
impl ConversationAdapter for DiscordAdapter {
    fn name(&self) -> &str {
        "discord"
    }

    async fn run(&self) -> AonyxResult<()> {
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;
        let bridge = Bridge {
            handler: Arc::clone(&self.handler),
            allowed: self.allowed.clone(),
        };
        tracing::info!("discord: gateway bot connecting; Ctrl-C to stop");
        let mut client = Client::builder(&self.token, intents)
            .event_handler(bridge)
            .await
            .map_err(|e| AonyxError::Adapter(format!("discord client: {e}")))?;
        client
            .start()
            .await
            .map_err(|e| AonyxError::Adapter(format!("discord: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::chunk_message;

    #[test]
    fn respects_discord_2000_cap() {
        let big = "z".repeat(5000);
        let parts = chunk_message(&big, 2000);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.chars().count() <= 2000));
        assert_eq!(parts.concat(), big);
    }
}
