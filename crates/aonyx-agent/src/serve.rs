//! `aonyx serve <channel>` — run a chat adapter bridged to the agent loop.
//!
//! The heavy platform SDKs sit behind cargo features; a build without the
//! feature still exposes the subcommand but prints how to get a build
//! that includes it.

/// Run the Telegram bot (`aonyx serve telegram`).
#[cfg(feature = "telegram")]
pub async fn telegram() -> anyhow::Result<()> {
    imp::telegram().await
}

/// Fallback when the binary was built without Telegram support.
#[cfg(not(feature = "telegram"))]
pub async fn telegram() -> anyhow::Result<()> {
    anyhow::bail!(
        "this build has no Telegram support — reinstall with \
         `cargo install aonyx-agent --features telegram`, or grab a release binary"
    )
}

/// Run the Discord bot (`aonyx serve discord`).
#[cfg(feature = "discord")]
pub async fn discord() -> anyhow::Result<()> {
    imp::discord().await
}

/// Fallback when the binary was built without Discord support.
#[cfg(not(feature = "discord"))]
pub async fn discord() -> anyhow::Result<()> {
    anyhow::bail!(
        "this build has no Discord support — reinstall with \
         `cargo install aonyx-agent --features discord`, or grab a release binary"
    )
}

#[cfg(any(feature = "telegram", feature = "discord"))]
mod imp {
    use std::collections::HashMap;
    use std::sync::Arc;

    use aonyx_adapters::{AgentHandler, ConversationAdapter};
    use aonyx_agent::AgentRunner;
    use aonyx_core::{Message, Role};
    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use crate::config::Config;

    /// Keep at most this many messages of per-chat history (plus the
    /// system prompt) so a long-lived bot conversation can't grow the
    /// request unbounded.
    const MAX_HISTORY: usize = 40;

    /// Bridges each inbound chat message to a shared [`AgentRunner`],
    /// keeping a separate transcript per conversation. Destructive tools
    /// are denied (the runner's default policy) — a remote chat must never
    /// be able to edit files or run shell commands on the host.
    struct RunnerHandler {
        runner: AgentRunner,
        system_prompt: Option<String>,
        chats: Mutex<HashMap<String, Vec<Message>>>,
    }

    impl RunnerHandler {
        fn seed(&self) -> Vec<Message> {
            match &self.system_prompt {
                Some(p) => vec![Message::new(Role::System, p.clone())],
                None => Vec::new(),
            }
        }
    }

    #[async_trait]
    impl AgentHandler for RunnerHandler {
        async fn handle(&self, chat_id: &str, text: &str) -> aonyx_core::Result<String> {
            let mut history = {
                let map = self.chats.lock().await;
                map.get(chat_id).cloned().unwrap_or_else(|| self.seed())
            };
            history.push(Message::new(Role::User, text));

            let result = self.runner.run(history).await?;
            let reply = last_assistant_text(&result.messages);

            let trimmed = trim_history(result.messages, MAX_HISTORY);
            self.chats.lock().await.insert(chat_id.to_string(), trimmed);
            Ok(reply)
        }
    }

    /// The last non-empty assistant message in a turn's log.
    fn last_assistant_text(messages: &[Message]) -> String {
        messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Assistant) && !m.content.trim().is_empty())
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "(no reply)".to_string())
    }

    /// Keep the leading system message (if any) plus the last `max`
    /// messages.
    fn trim_history(mut msgs: Vec<Message>, max: usize) -> Vec<Message> {
        if msgs.len() <= max {
            return msgs;
        }
        let keep_system = msgs.first().is_some_and(|m| matches!(m.role, Role::System));
        let start = msgs.len() - max;
        if keep_system {
            let system = msgs[0].clone();
            let mut out = Vec::with_capacity(max + 1);
            out.push(system);
            out.extend_from_slice(&msgs[start..]);
            out
        } else {
            msgs.split_off(start)
        }
    }

    /// Build the shared agent handler (provider + tools + memory palace +
    /// skills) from the current config and working directory.
    fn build_handler(config: &Config) -> anyhow::Result<Arc<RunnerHandler>> {
        let provider = crate::build_provider(config)?;
        let registry = crate::build_serve_registry()?;
        let project = crate::project_slug(&std::env::current_dir()?);
        let runner = AgentRunner::new(provider, registry, config.model.clone())
            .with_max_iterations(config.max_iterations)
            .with_skills(crate::load_all_skills())
            .with_project(project);
        Ok(Arc::new(RunnerHandler {
            runner,
            system_prompt: config.system_prompt.clone(),
            chats: Mutex::new(HashMap::new()),
        }))
    }

    fn announce(channel: &str, allowed: usize, setup_cmd: &str) {
        if allowed == 0 {
            eprintln!(
                "aonyx: {channel} bot starting — OPEN to all chats \
                 (lock it down with `{setup_cmd}`). Ctrl-C to stop."
            );
        } else {
            eprintln!("aonyx: {channel} bot starting — {allowed} allowed. Ctrl-C to stop.");
        }
    }

    #[cfg(feature = "telegram")]
    pub async fn telegram() -> anyhow::Result<()> {
        use aonyx_adapters::telegram::TelegramAdapter;
        let config = Config::load_or_init()?;
        let token = crate::resolve_key(&None, "TELEGRAM_BOT_TOKEN", "telegram_bot_token").map_err(
            |_| {
                anyhow::anyhow!(
                    "no Telegram bot token — run `aonyx setup telegram`, or export TELEGRAM_BOT_TOKEN"
                )
            },
        )?;
        let handler = build_handler(&config)?;
        let allowed = config.telegram_allowed_chats.clone();
        announce("Telegram", allowed.len(), "aonyx setup telegram");
        TelegramAdapter::new(token, allowed, handler)
            .run()
            .await
            .map_err(|e| anyhow::anyhow!("telegram: {e}"))
    }

    #[cfg(feature = "discord")]
    pub async fn discord() -> anyhow::Result<()> {
        use aonyx_adapters::discord::DiscordAdapter;
        let config = Config::load_or_init()?;
        let token =
            crate::resolve_key(&None, "DISCORD_BOT_TOKEN", "discord_bot_token").map_err(|_| {
                anyhow::anyhow!(
                    "no Discord bot token — run `aonyx setup discord`, or export DISCORD_BOT_TOKEN"
                )
            })?;
        let handler = build_handler(&config)?;
        let allowed = config.discord_allowed_channels.clone();
        announce("Discord", allowed.len(), "aonyx setup discord");
        DiscordAdapter::new(token, allowed, handler)
            .run()
            .await
            .map_err(|e| anyhow::anyhow!("discord: {e}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn msg(role: Role, c: &str) -> Message {
            Message::new(role, c)
        }

        #[test]
        fn trim_keeps_system_and_tail() {
            let mut v = vec![msg(Role::System, "sys")];
            for i in 0..100 {
                v.push(msg(Role::User, &format!("u{i}")));
            }
            let out = trim_history(v, 10);
            assert_eq!(out.len(), 11); // system + 10
            assert!(matches!(out[0].role, Role::System));
            assert_eq!(out[0].content, "sys");
            assert_eq!(out.last().unwrap().content, "u99");
        }

        #[test]
        fn trim_noop_when_small() {
            let v = vec![msg(Role::User, "a"), msg(Role::Assistant, "b")];
            assert_eq!(trim_history(v.clone(), 40).len(), v.len());
        }

        #[test]
        fn last_assistant_text_picks_final_nonempty() {
            let v = vec![
                msg(Role::User, "q"),
                msg(Role::Assistant, "first"),
                msg(Role::User, "q2"),
                msg(Role::Assistant, "final"),
            ];
            assert_eq!(last_assistant_text(&v), "final");
        }
    }
}
