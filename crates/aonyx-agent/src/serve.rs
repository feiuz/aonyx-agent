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

/// Run the OpenAI-compatible HTTP server (`aonyx serve openai`).
#[cfg(feature = "openai-server")]
pub async fn openai(port: u16, token: Option<String>) -> anyhow::Result<()> {
    imp::openai(port, token).await
}

/// Fallback when the binary was built without OpenAI-server support.
#[cfg(not(feature = "openai-server"))]
pub async fn openai(_port: u16, _token: Option<String>) -> anyhow::Result<()> {
    anyhow::bail!(
        "this build has no OpenAI-server support — reinstall with \
         `cargo install aonyx-agent --features openai-server`, or grab a release binary"
    )
}

/// Run the REST + WebSocket automation API (`aonyx serve api`, Vague 4).
#[cfg(feature = "api")]
pub async fn api(port: u16, token: Option<String>, bind: String) -> anyhow::Result<()> {
    api_imp::run(port, token, bind).await
}

/// Fallback when the binary was built without API support.
#[cfg(not(feature = "api"))]
pub async fn api(_port: u16, _token: Option<String>, _bind: String) -> anyhow::Result<()> {
    anyhow::bail!(
        "this build has no API support — reinstall with \
         `cargo install aonyx-agent --features api`, or grab a release binary"
    )
}

/// `aonyx serve api` — build the [`aonyx_api`] state over the real agent
/// loop, memory palace, and session store, then serve it.
#[cfg(feature = "api")]
mod api_imp {
    use std::sync::Arc;

    use aonyx_agent::{AgentRunner, ApprovalPolicy, TurnEvent};
    use aonyx_api::{
        ApiAgent, ApiState, AuthConfig, ConfigInfo, ServerInfo, SkillInfo, StreamFrame, ToolInfo,
    };
    use aonyx_core::{Message, SafetyClass};
    use aonyx_memory::{Palace, SqliteSessionStore};
    use async_trait::async_trait;
    use tokio::sync::mpsc;

    use crate::config::Config;

    /// Adapts the binary's [`AgentRunner`] (+ tool/skill/config snapshots) to
    /// the [`ApiAgent`] trait the API layer drives.
    struct ApiRunner {
        runner: AgentRunner,
        tools: Vec<ToolInfo>,
        skills: Vec<SkillInfo>,
        config: ConfigInfo,
    }

    #[async_trait]
    impl ApiAgent for ApiRunner {
        async fn run_turn(&self, history: Vec<Message>) -> aonyx_core::Result<Vec<Message>> {
            Ok(self.runner.run(history).await?.messages)
        }

        async fn run_turn_streaming(
            &self,
            history: Vec<Message>,
            tx: mpsc::Sender<StreamFrame>,
        ) -> aonyx_core::Result<Vec<Message>> {
            // Bridge the runner's TurnEvent stream onto the API's StreamFrame.
            let (etx, mut erx) = mpsc::channel::<TurnEvent>(128);
            let forward = async move {
                while let Some(ev) = erx.recv().await {
                    if let Some(frame) = map_event(ev) {
                        if tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                }
            };
            let drive = self.runner.run_streaming(history, etx);
            let (res, _) = tokio::join!(drive, forward);
            Ok(res?.messages)
        }

        fn tools(&self) -> Vec<ToolInfo> {
            self.tools.clone()
        }

        fn skills(&self) -> Vec<SkillInfo> {
            self.skills.clone()
        }

        fn config(&self) -> ConfigInfo {
            self.config.clone()
        }
    }

    fn class_str(c: SafetyClass) -> String {
        match c {
            SafetyClass::Safe => "safe",
            SafetyClass::Caution => "caution",
            SafetyClass::Destructive => "destructive",
        }
        .to_string()
    }

    /// Map a runner [`TurnEvent`] onto an API [`StreamFrame`]. The terminal
    /// `AssistantMessageEnd`/`Done` are dropped — the API layer emits its own
    /// `Done` after persisting.
    fn map_event(ev: TurnEvent) -> Option<StreamFrame> {
        Some(match ev {
            TurnEvent::AssistantDelta(text) => StreamFrame::Delta { text },
            TurnEvent::ToolStart { name, args, class } => StreamFrame::ToolStart {
                name,
                args,
                class: class_str(class),
            },
            TurnEvent::ToolEnd { name, ok, summary } => StreamFrame::ToolEnd { name, ok, summary },
            TurnEvent::ToolRejected { name, class } => StreamFrame::ToolRejected {
                name,
                class: class_str(class),
            },
            TurnEvent::IterationStart(n) => StreamFrame::Iteration { n: n as u32 },
            TurnEvent::AssistantMessageEnd | TurnEvent::Done { .. } => return None,
        })
    }

    fn tool_infos(registry: &aonyx_tools::ToolRegistry) -> Vec<ToolInfo> {
        let names: Vec<String> = registry.names().map(|s| s.to_string()).collect();
        names
            .into_iter()
            .filter_map(|name| registry.get(&name))
            .map(|h| {
                let schema = h.schema();
                let description = schema
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                ToolInfo {
                    name: h.name().to_string(),
                    description,
                    class: class_str(h.classify()),
                    schema,
                }
            })
            .collect()
    }

    fn skill_infos(skills: &[aonyx_skills::Skill]) -> Vec<SkillInfo> {
        skills
            .iter()
            .map(|s| {
                let mut triggers = Vec::new();
                if s.trigger.always_on {
                    triggers.push("always-on".to_string());
                }
                if s.trigger.manual {
                    triggers.push("manual".to_string());
                }
                triggers.extend(s.trigger.keywords.iter().cloned());
                if let Some(p) = &s.trigger.project_matches {
                    triggers.push(format!("project~/{p}/"));
                }
                SkillInfo {
                    id: s.id.clone(),
                    description: s.name.clone(),
                    triggers,
                }
            })
            .collect()
    }

    fn api_features() -> Vec<String> {
        vec![
            "memory".to_string(),
            "openai-compat".to_string(),
            "sessions".to_string(),
            "streaming".to_string(),
            "tools".to_string(),
        ]
    }

    pub async fn run(port: u16, token: Option<String>, bind: String) -> anyhow::Result<()> {
        let config = Config::load_or_init()?;
        let cwd = std::env::current_dir()?;
        let project = crate::project_slug(&cwd);

        // Build the live components (provider + tools + skills) exactly like
        // the chat adapters do.
        let provider = crate::build_provider(&config)?;
        let registry = crate::build_serve_registry().await?;
        let skills = crate::load_all_skills();

        // Snapshot metadata BEFORE the registry/skills move into the runner.
        let tools = tool_infos(&registry);
        let skill_list = skill_infos(&skills);
        let config_info = ConfigInfo {
            provider: config.provider.clone(),
            model: config.model.clone(),
            max_iterations: config.max_iterations,
            skill_autogen: config.skill_autogen,
        };

        let runner = AgentRunner::new(provider, registry, config.model.clone())
            .with_max_iterations(config.max_iterations)
            .with_approval(ApprovalPolicy::DenyDestructive)
            .with_skills(skills)
            .with_project(&project);

        let api_runner = ApiRunner {
            runner,
            tools,
            skills: skill_list,
            config: config_info,
        };

        // Memory palace (current dir) + cross-run session store.
        let palace = Palace::open(Palace::default_project_dir(&cwd))?;
        let sessions = SqliteSessionStore::open(Config::config_dir()?.join("sessions.db"))?;

        // Token: flag → keyring → env.
        let token = token
            .or_else(|| crate::secrets::get("api_token"))
            .or_else(|| std::env::var("AONYX_API_TOKEN").ok());

        // A non-loopback bind without a token is refused (FR-AX4).
        let loopback = matches!(bind.as_str(), "127.0.0.1" | "::1" | "localhost");
        if !loopback && token.is_none() {
            anyhow::bail!(
                "refusing to bind {bind} without a token — pass --token, set \
                 AONYX_API_TOKEN, run `aonyx setup`-stored `api_token`, or bind 127.0.0.1"
            );
        }

        let info = ServerInfo::new(
            config.provider.clone(),
            config.model.clone(),
            api_features(),
        );
        // Destructive tools are denied at the loop level (DenyDestructive);
        // the direct tool-invoke endpoint is not exposed, so `false` here.
        let auth = AuthConfig::new(token.clone(), false);
        let state = ApiState::new(
            auth,
            info,
            Arc::new(sessions),
            Arc::new(palace),
            Arc::new(api_runner),
            project,
        );

        let addr = format!("{bind}:{port}");
        if token.is_some() {
            eprintln!(
                "aonyx: API on http://{addr}/v1 (bearer auth ON) — \
                 docs at /v1/openapi.json. Ctrl-C to stop."
            );
        } else {
            eprintln!(
                "aonyx: API on http://{addr}/v1 (no auth — keep it on localhost) — \
                 docs at /v1/openapi.json. Ctrl-C to stop."
            );
        }
        aonyx_api::serve(state, &addr)
            .await
            .map_err(|e| anyhow::anyhow!("api serve: {e}"))
    }
}

#[cfg(any(feature = "telegram", feature = "discord", feature = "openai-server"))]
mod imp {
    use std::collections::HashMap;
    use std::sync::Arc;

    use aonyx_adapters::{AgentHandler, StreamEvent};
    use aonyx_agent::{AgentRunner, TurnEvent};
    use aonyx_core::{Message, Role};
    use async_trait::async_trait;
    use tokio::sync::{mpsc, Mutex};

    use crate::config::Config;

    /// Keep at most this many messages of per-chat history (plus the
    /// system prompt) so a long-lived bot conversation can't grow the
    /// request unbounded.
    const MAX_HISTORY: usize = 40;

    /// Bridges inbound messages to a shared [`AgentRunner`]. Chat adapters
    /// keep a separate transcript per conversation; the OpenAI server is
    /// stateless. Destructive tools are denied (the runner's default
    /// policy) — a remote client must never edit files or run shell on the
    /// host.
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

        async fn complete(&self, messages: Vec<(String, String)>) -> aonyx_core::Result<String> {
            // Stateless: the caller (OpenAI server) owns the history, so we
            // run one turn over exactly the messages it sent.
            let msgs: Vec<Message> = messages
                .into_iter()
                .map(|(role, content)| Message::new(role_from_str(&role), content))
                .collect();
            let result = self.runner.run(msgs).await?;
            Ok(last_assistant_text(&result.messages))
        }

        async fn handle_stream(
            &self,
            chat_id: &str,
            text: &str,
            out: mpsc::Sender<StreamEvent>,
        ) -> aonyx_core::Result<()> {
            let mut history = {
                let map = self.chats.lock().await;
                map.get(chat_id).cloned().unwrap_or_else(|| self.seed())
            };
            history.push(Message::new(Role::User, text));

            // Drive the runner's streaming loop; translate its internal
            // TurnEvents into adapter-level StreamEvents as they arrive. Tool
            // calls surface as a transient status line (never raw tool JSON).
            let (etx, mut erx) = mpsc::channel::<TurnEvent>(256);
            let fwd = out.clone();
            let forward = async move {
                while let Some(ev) = erx.recv().await {
                    let mapped = match ev {
                        TurnEvent::AssistantDelta(t) => Some(StreamEvent::Delta(t)),
                        TurnEvent::ToolStart { name, .. } => {
                            Some(StreamEvent::Status(tool_status(&name)))
                        }
                        _ => None,
                    };
                    if let Some(se) = mapped {
                        if fwd.send(se).await.is_err() {
                            break; // adapter hung up
                        }
                    }
                }
            };

            let drive = self.runner.run_streaming(history, etx);
            let (res, _) = tokio::join!(drive, forward);

            // Persist trimmed history + emit the authoritative final reply.
            // On error, surface it as the Final text so the adapter shows it.
            match res {
                Ok(result) => {
                    let reply = last_assistant_text(&result.messages);
                    let trimmed = trim_history(result.messages, MAX_HISTORY);
                    self.chats.lock().await.insert(chat_id.to_string(), trimmed);
                    let _ = out.send(StreamEvent::Final(reply)).await;
                    Ok(())
                }
                Err(e) => {
                    let _ = out.send(StreamEvent::Final(format!("⚠ {e}"))).await;
                    Err(e)
                }
            }
        }
    }

    /// Map an OpenAI role string to an Aonyx [`Role`].
    fn role_from_str(role: &str) -> Role {
        match role {
            "system" => Role::System,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            _ => Role::User,
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

    /// A short, friendly status line for a tool call — shown transiently in a
    /// streamed reply while the tool runs (never the raw tool arguments).
    fn tool_status(name: &str) -> String {
        let n = name.to_ascii_lowercase();
        if n.contains("rag") || n.contains("search") || n.contains("memory") || n.contains("recall")
        {
            "🔍 recherche dans la mémoire…".to_string()
        } else if n.contains("read") || n.contains("view") || n.contains("get") || n.contains("list")
        {
            "📄 lecture…".to_string()
        } else if n.contains("write") || n.contains("edit") || n.contains("append") {
            "✏️ écriture…".to_string()
        } else {
            format!("🔧 {name}…")
        }
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
    async fn build_handler(config: &Config) -> anyhow::Result<Arc<RunnerHandler>> {
        let provider = crate::build_provider(config)?;
        let registry = crate::build_serve_registry().await?;
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

    #[cfg(any(feature = "telegram", feature = "discord"))]
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
        use aonyx_adapters::{telegram::TelegramAdapter, ConversationAdapter};
        let config = Config::load_or_init()?;
        let token = crate::resolve_key(&None, "TELEGRAM_BOT_TOKEN", "telegram_bot_token").map_err(
            |_| {
                anyhow::anyhow!(
                    "no Telegram bot token — run `aonyx setup telegram`, or export TELEGRAM_BOT_TOKEN"
                )
            },
        )?;
        let handler = build_handler(&config).await?;
        let allowed = config.telegram_allowed_chats.clone();
        announce("Telegram", allowed.len(), "aonyx setup telegram");
        TelegramAdapter::new(token, allowed, handler)
            .run()
            .await
            .map_err(|e| anyhow::anyhow!("telegram: {e}"))
    }

    #[cfg(feature = "discord")]
    pub async fn discord() -> anyhow::Result<()> {
        use aonyx_adapters::{discord::DiscordAdapter, ConversationAdapter};
        let config = Config::load_or_init()?;
        let token =
            crate::resolve_key(&None, "DISCORD_BOT_TOKEN", "discord_bot_token").map_err(|_| {
                anyhow::anyhow!(
                    "no Discord bot token — run `aonyx setup discord`, or export DISCORD_BOT_TOKEN"
                )
            })?;
        let handler = build_handler(&config).await?;
        let allowed = config.discord_allowed_channels.clone();
        announce("Discord", allowed.len(), "aonyx setup discord");
        DiscordAdapter::new(token, allowed, handler)
            .run()
            .await
            .map_err(|e| anyhow::anyhow!("discord: {e}"))
    }

    #[cfg(feature = "openai-server")]
    pub async fn openai(port: u16, token: Option<String>) -> anyhow::Result<()> {
        use aonyx_adapters::openai_server::OpenAiServer;
        let config = Config::load_or_init()?;
        let handler = build_handler(&config).await?;
        let token = token.or_else(|| std::env::var("AONYX_OPENAI_TOKEN").ok());
        let addr = format!("127.0.0.1:{port}");
        if token.is_some() {
            eprintln!(
                "aonyx: OpenAI-compatible server on http://{addr}/v1 (bearer auth ON). Ctrl-C to stop."
            );
        } else {
            eprintln!(
                "aonyx: OpenAI-compatible server on http://{addr}/v1 \
                 (no auth — keep it on localhost). Ctrl-C to stop."
            );
        }
        OpenAiServer::new(addr, token, handler)
            .run()
            .await
            .map_err(|e| anyhow::anyhow!("openai-server: {e}"))
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

        #[test]
        fn role_mapping() {
            assert!(matches!(role_from_str("system"), Role::System));
            assert!(matches!(role_from_str("assistant"), Role::Assistant));
            assert!(matches!(role_from_str("tool"), Role::Tool));
            assert!(matches!(role_from_str("user"), Role::User));
            assert!(matches!(role_from_str("whatever"), Role::User));
        }
    }
}
