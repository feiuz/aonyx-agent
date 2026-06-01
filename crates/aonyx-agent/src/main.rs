//! `aonyx` — the Aonyx Agent command-line binary.
//!
//! ```text
//! aonyx                  open an interactive session in the current dir
//! aonyx setup            interactive wizard: provider, key (keyring), model
//! aonyx new <path>       start a new session scoped to <path>
//! aonyx resume [id]      resume the latest session, or one by id-prefix
//! aonyx config <subcmd>  show / locate the config file
//! aonyx memory <subcmd>  stats / hybrid-search the palace
//! aonyx skills <subcmd>  list the active skill catalogue
//! aonyx mcp <subcmd>     run the MCP server (stdio or HTTP)
//! aonyx serve <channel>  run an adapter: telegram, discord, or openai (HTTP)
//! ```

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use aonyx_core::LlmProvider;
use aonyx_llm::anthropic::AnthropicProvider;
use aonyx_llm::lm_studio::LM_STUDIO_DEFAULT_BASE_URL;
use aonyx_llm::openai::OPENAI_BASE_URL;
use aonyx_llm::{
    ClaudeCodeProvider, OllamaProvider, OpenAiCompatProvider, CLAUDE_DEFAULT_BIN,
    OLLAMA_DEFAULT_BASE_URL,
};
use aonyx_memory::{Palace, SessionStore, SqliteSessionStore};
use clap::{Parser, Subcommand};

mod config;
mod images;
mod pricing;
mod secrets;
mod serve;
mod session;
mod setup;
mod theme;
mod tui;

use config::Config;
use session::InteractiveSession;

/// Aonyx Agent — the agent with a real memory palace.
#[derive(Debug, Parser)]
#[command(name = "aonyx", version, about, long_about = None)]
struct Cli {
    /// Verbose logging (`debug` level).
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Open the new full-screen TUI (Phase B preview) instead of the
    /// legacy line-based REPL.
    #[arg(long)]
    tui: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start a new session scoped to a project path.
    New {
        /// Project directory (default: current directory).
        path: Option<PathBuf>,
    },
    /// Resume a previous session.
    Resume {
        /// Session id (default: last).
        id: Option<String>,
    },
    /// Manage configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Inspect the memory palace.
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Manage skills.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// Run or connect to an MCP server.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Interactive configuration wizard (provider, credentials, model).
    Setup {
        #[command(subcommand)]
        action: Option<SetupAction>,
    },
    /// Run a chat adapter bridged to the agent (Telegram, …).
    Serve {
        #[command(subcommand)]
        channel: ServeChannel,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Print the config path and contents.
    Show,
    /// Print the config path.
    Path,
}

#[derive(Debug, Subcommand)]
enum MemoryAction {
    /// Show counts and health for the palace.
    Stats,
    /// Hybrid-search across chunks.
    Search { query: String },
}

#[derive(Debug, Subcommand)]
enum SkillsAction {
    /// List known skills.
    List,
}

#[derive(Debug, Subcommand)]
enum McpAction {
    /// Serve the Aonyx MCP server.
    Serve {
        /// TCP port; omit for stdio.
        #[arg(short, long)]
        port: Option<u16>,
        /// Require this bearer token on the HTTP transport (Phase PP).
        /// Falls back to $AONYX_MCP_TOKEN. Ignored for stdio.
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SetupAction {
    /// Configure the LLM provider — the default when no subcommand is given.
    Provider,
    /// Configure the Telegram bot (token in the keyring + allowed chats).
    Telegram,
    /// Configure the Discord bot (token in the keyring + allowed channels).
    Discord,
}

#[derive(Debug, Subcommand)]
enum ServeChannel {
    /// Run the Telegram bot (needs the `telegram` build feature).
    Telegram,
    /// Run the Discord bot (needs the `discord` build feature).
    Discord,
    /// Run the OpenAI-compatible HTTP server (needs the `openai-server` feature).
    Openai {
        /// TCP port to listen on (binds localhost).
        #[arg(short, long, default_value_t = 8787)]
        port: u16,
        /// Require this bearer token (falls back to $AONYX_OPENAI_TOKEN).
        #[arg(long)]
        token: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let use_tui = cli.tui;
    match cli.command {
        None => start_interactive(None, use_tui, StartMode::Default).await,
        Some(Command::New { path }) => start_interactive(path, use_tui, StartMode::Default).await,
        Some(Command::Resume { id }) => {
            // Phase QQ — `aonyx resume` reopens the latest session for the
            // current dir; `aonyx resume <id-prefix>` reopens a specific
            // one (across projects).
            let mode = match id {
                Some(prefix) => StartMode::ResumeById(prefix),
                None => StartMode::ResumeLatest,
            };
            start_interactive(None, use_tui, mode).await
        }
        Some(Command::Config { action }) => handle_config(action),
        Some(Command::Memory { action }) => handle_memory(action).await,
        Some(Command::Skills { action }) => match action {
            SkillsAction::List => {
                handle_skills_list();
                Ok(())
            }
        },
        Some(Command::Setup { action }) => match action {
            None | Some(SetupAction::Provider) => setup::run_provider_wizard().await,
            Some(SetupAction::Telegram) => setup::run_telegram_wizard().await,
            Some(SetupAction::Discord) => setup::run_discord_wizard().await,
        },
        Some(Command::Serve { channel }) => match channel {
            ServeChannel::Telegram => serve::telegram().await,
            ServeChannel::Discord => serve::discord().await,
            ServeChannel::Openai { port, token } => serve::openai(port, token).await,
        },
        Some(Command::Mcp { action }) => match action {
            McpAction::Serve { port, token } => {
                // Phase HH/NN — expose the built-in tools plus the
                // palace-backed `memory_*` tools (scoped to the current
                // directory's palace) so remote clients (Claude Code,
                // Cursor, …) can read and write *this project's* memory.
                let registry = build_serve_registry()?;
                match port {
                    // Phase OO — Streamable HTTP transport on a TCP port.
                    Some(p) => {
                        // Phase PP — optional bearer auth (flag or env).
                        let token = token.or_else(|| std::env::var("AONYX_MCP_TOKEN").ok());
                        let auth = if token.is_some() {
                            "bearer auth ON"
                        } else {
                            "no auth — bind localhost only"
                        };
                        let addr = format!("127.0.0.1:{p}");
                        eprintln!(
                            "aonyx: MCP server ready on http://{addr} \
                             (fs / bash / git / web / memory_*) [{auth}]"
                        );
                        aonyx_mcp::server::serve_http(registry, &addr, token)
                            .await
                            .map_err(|e| anyhow::anyhow!("mcp serve http: {e}"))
                    }
                    // Default: stdio, blocks until stdin closes (HH).
                    None => {
                        eprintln!(
                            "aonyx: MCP server ready on stdio \
                             (fs / bash / git / web / memory_*)"
                        );
                        aonyx_mcp::server::serve_stdio(registry)
                            .await
                            .map_err(|e| anyhow::anyhow!("mcp serve: {e}"))
                    }
                }
            }
        },
    }
}

fn init_tracing(verbose: bool) {
    let level = if verbose { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    // Logs go to stderr so they never corrupt stdout — which doubles as
    // the JSON-RPC channel under `aonyx mcp serve` (Phase HH).
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

/// How a session launches (Phase QQ). `Default` (and `aonyx new`)
/// restores the most recent session for the project or creates one;
/// `ResumeLatest` is `aonyx resume` (same, but announces it);
/// `ResumeById` is `aonyx resume <id-prefix>`, resolving a specific
/// past session across all projects.
enum StartMode {
    Default,
    ResumeLatest,
    ResumeById(String),
}

async fn start_interactive(
    project_path: Option<PathBuf>,
    use_tui: bool,
    mode: StartMode,
) -> anyhow::Result<()> {
    let config = Config::load_or_init()?;

    let project_root = match project_path {
        Some(p) => {
            if !p.exists() {
                std::fs::create_dir_all(&p)?;
            }
            std::fs::canonicalize(&p)?
        }
        None => std::env::current_dir()?,
    };

    let provider = build_provider(&config)?;
    let palace_dir = Palace::default_project_dir(&project_root);
    let palace = Palace::open(&palace_dir)?;
    let project_slug = project_slug(&project_root);

    let skills = load_all_skills();

    // Cross-run session storage at ~/.aonyx/sessions.db.
    let sessions_db_path = Config::config_dir()?.join("sessions.db");
    let session_store = SqliteSessionStore::open(&sessions_db_path)?;

    // Build the initial transcript (system prompt only) — the session
    // restore logic will replace it with persisted messages when available.
    let initial_messages: Vec<aonyx_core::Message> = config
        .system_prompt
        .as_ref()
        .map(|p| {
            vec![aonyx_core::Message::new(
                aonyx_core::Role::System,
                p.clone(),
            )]
        })
        .unwrap_or_default();

    // Phase QQ — resolve which session to open per the launch mode.
    let mut project_slug = project_slug;
    let restored = match &mode {
        StartMode::ResumeById(prefix) => {
            let mut matches = session_store.find_by_id_prefix(prefix.trim(), 5).await?;
            if matches.is_empty() {
                anyhow::bail!("no session matches id prefix '{}'", prefix.trim());
            }
            if matches.len() > 1 {
                eprintln!(
                    "aonyx: ambiguous prefix '{}' — {} matches:",
                    prefix.trim(),
                    matches.len()
                );
                for r in &matches {
                    let short: String = r.id.to_string().chars().take(8).collect();
                    eprintln!(
                        "  [{short}] {} · {} · {} turns",
                        r.project, r.title, r.turns
                    );
                }
                anyhow::bail!("refine the id prefix");
            }
            let rec = matches.remove(0);
            // Adopt the resumed session's project so its palace + slug
            // line up with the transcript we're reopening.
            project_slug = rec.project.clone();
            Some(rec)
        }
        StartMode::ResumeLatest | StartMode::Default => session_store.latest(&project_slug).await?,
    };
    if let Some(s) = &restored {
        let short: String = s.id.to_string().chars().take(8).collect();
        eprintln!(
            "aonyx: resuming session [{short}] · {} · {} turns",
            s.project, s.turns
        );
    } else if matches!(mode, StartMode::ResumeLatest) {
        eprintln!("aonyx: no prior session for '{project_slug}' — starting fresh");
    }
    let (session_id, session_messages, session_turns) = match restored {
        Some(s) => (s.id, s.messages, s.turns),
        None => {
            let created = session_store
                .create(&project_slug, initial_messages.clone())
                .await?;
            (created.id, created.messages, 0)
        }
    };

    // Build the tool registry and fold in any configured MCP servers
    // (Phase GG). A server that fails to start just logs and is
    // skipped — it must never block the session.
    let mut tool_registry = aonyx_tools::ToolRegistry::default_set();
    for srv in &config.mcp_servers {
        // `url` selects the HTTP transport (Phase II); otherwise fall
        // back to stdio via `command` (Phase GG).
        let outcome = if let Some(url) = &srv.url {
            aonyx_mcp::client::connect_http_and_register(
                &mut tool_registry,
                &srv.name,
                url,
                srv.bearer_token.clone(),
            )
            .await
        } else if let Some(command) = &srv.command {
            aonyx_mcp::client::connect_and_register(
                &mut tool_registry,
                &srv.name,
                command,
                &srv.args,
            )
            .await
        } else {
            Err(aonyx_core::AonyxError::Mcp(format!(
                "server '{}' has neither `command` nor `url`",
                srv.name
            )))
        };
        match outcome {
            Ok(n) => eprintln!(
                "aonyx: MCP '{}' connected — {n} tool(s) registered",
                srv.name
            ),
            Err(e) => eprintln!("aonyx: MCP '{}' failed: {e}", srv.name),
        }
    }

    // Phase WW — fold in user Lua plugins from ~/.aonyx/plugins/.
    register_plugins(&mut tool_registry);

    // Phase OO — seed the always-allow approval set from persisted config
    // so tools the user chose to always allow skip the prompt this run.
    tui::seed_tool_approvals(&config.tool_approvals);

    if use_tui {
        return tui::run(
            provider,
            palace,
            config.model.clone(),
            config.max_iterations,
            config.system_prompt.clone(),
            project_slug,
            skills,
            config.provider.clone(),
            session_store,
            session_id,
            session_messages,
            session_turns,
            config.theme.clone(),
            config.custom_theme.as_ref().map(|c| c.to_rgb_fields()),
            config.show_thinking,
            config.desktop_notifications,
            config.auto_compact,
            config.auto_compact_threshold,
            tool_registry,
        )
        .await;
    }

    let mut session = InteractiveSession::new(
        provider,
        palace,
        config.model.clone(),
        config.max_iterations,
        config.system_prompt.clone(),
        project_slug,
        skills,
        config.provider.clone(),
    );
    session.run().await
}

fn resolve_key(
    stored: &Option<String>,
    env_var: &str,
    config_field: &str,
) -> anyhow::Result<String> {
    // Resolution order: explicit value in config.toml → OS keyring (where
    // `aonyx setup` stores it) → environment variable.
    stored
        .clone()
        .or_else(|| secrets::get(config_field))
        .or_else(|| std::env::var(env_var).ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{config_field} missing — run `aonyx setup`, set it in ~/.aonyx/config.toml, or export {env_var}"
            )
        })
}

fn build_provider(config: &Config) -> anyhow::Result<Arc<dyn LlmProvider>> {
    match config.provider.as_str() {
        "anthropic" => {
            let key = resolve_key(
                &config.anthropic_api_key,
                "ANTHROPIC_API_KEY",
                "anthropic_api_key",
            )?;
            Ok(Arc::new(AnthropicProvider::new(key)))
        }
        "openai" => {
            let key = resolve_key(&config.openai_api_key, "OPENAI_API_KEY", "openai_api_key")?;
            let base = config
                .openai_base_url
                .clone()
                .unwrap_or_else(|| OPENAI_BASE_URL.to_string());
            Ok(Arc::new(OpenAiCompatProvider::new("openai", key, base)))
        }
        "openrouter" => {
            let key = resolve_key(
                &config.openrouter_api_key,
                "OPENROUTER_API_KEY",
                "openrouter_api_key",
            )?;
            let mut p = OpenAiCompatProvider::new(
                "openrouter",
                key,
                aonyx_llm::openrouter::OPENROUTER_BASE_URL,
            );
            if let Some(referer) = &config.openrouter_referer {
                p = p.with_header("HTTP-Referer", referer);
            }
            if let Some(title) = &config.openrouter_title {
                p = p.with_header("X-Title", title);
            }
            Ok(Arc::new(p))
        }
        "ollama" => {
            let base = config
                .ollama_base_url
                .clone()
                .unwrap_or_else(|| OLLAMA_DEFAULT_BASE_URL.to_string());
            Ok(Arc::new(OllamaProvider::with_base_url(base)))
        }
        "lm-studio" | "lm_studio" => {
            let base = config
                .lm_studio_base_url
                .clone()
                .unwrap_or_else(|| LM_STUDIO_DEFAULT_BASE_URL.to_string());
            Ok(Arc::new(OpenAiCompatProvider::new(
                "lm-studio",
                String::new(),
                base,
            )))
        }
        "claude-code" | "claude_code" => {
            let bin = config
                .claude_code_binary
                .clone()
                .unwrap_or_else(|| CLAUDE_DEFAULT_BIN.to_string());
            let mut p = ClaudeCodeProvider::new().with_binary(bin);
            if !config.claude_code_extra_args.is_empty() {
                p = p.with_extra_args(config.claude_code_extra_args.clone());
            }
            Ok(Arc::new(p))
        }
        other => Err(anyhow::anyhow!(
            "provider '{other}' is not supported. \
             Available: anthropic, openai, openrouter, ollama, lm-studio, claude-code."
        )),
    }
}

fn project_slug(root: &std::path::Path) -> String {
    root.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "session".to_string())
}

/// Build the tool catalogue served by `aonyx mcp serve` (Phase NN).
///
/// Starts from the static [`ToolRegistry::default_set`](aonyx_tools::ToolRegistry::default_set)
/// (fs / bash / git / web) and folds in the three palace-backed
/// `memory_*` tools, scoped to the **current directory**'s palace — so a
/// remote MCP client operates on the same memory the local TUI does.
fn build_serve_registry() -> anyhow::Result<aonyx_tools::ToolRegistry> {
    let project_root = std::env::current_dir()?;
    let palace_dir = Palace::default_project_dir(&project_root);
    let palace = Palace::open(&palace_dir)?;
    let slug = project_slug(&project_root);

    let mut registry = aonyx_tools::ToolRegistry::default_set();
    registry.register(Arc::new(aonyx_tools::memory::MemorySearch::new(
        palace.clone(),
    )));
    registry.register(Arc::new(aonyx_tools::memory::MemoryDiaryAppend::new(
        palace.clone(),
        slug,
    )));
    registry.register(Arc::new(aonyx_tools::memory::MemoryKgQuery::new(
        palace.kg.clone(),
    )));
    register_plugins(&mut registry);
    Ok(registry)
}

/// Fold any user Lua plugins (`~/.aonyx/plugins/*.lua`) into `registry`
/// (Phase WW). No-op unless built with the `lua-plugins` feature.
#[cfg(feature = "lua-plugins")]
fn register_plugins(registry: &mut aonyx_tools::ToolRegistry) {
    let dir = match Config::config_dir() {
        Ok(d) => d.join("plugins"),
        Err(_) => return,
    };
    let tools = aonyx_tools::plugins::load_plugins(&dir);
    if !tools.is_empty() {
        eprintln!(
            "aonyx: loaded {} Lua plugin tool(s) from {}",
            tools.len(),
            dir.display()
        );
    }
    for tool in tools {
        registry.register(tool);
    }
}

/// No-op when the `lua-plugins` feature is disabled.
#[cfg(not(feature = "lua-plugins"))]
fn register_plugins(_registry: &mut aonyx_tools::ToolRegistry) {}

/// Build the active skill catalogue: the four built-ins plus any
/// user-authored `SKILL.md` / `*.skill.md` files under
/// `~/.aonyx/skills/` (Phase DD). User skills sharing a built-in `id`
/// override it, so a user can customise a shipped skill by dropping a
/// same-id file in their config dir.
fn load_all_skills() -> Vec<aonyx_skills::Skill> {
    let builtins = aonyx_skills::builtin_skills();
    let user_dir = match Config::config_dir() {
        Ok(d) => d.join("skills"),
        Err(_) => return builtins,
    };
    if !user_dir.is_dir() {
        return builtins;
    }
    match aonyx_skills::SkillLoader::load_dir(&user_dir) {
        Ok(user_skills) => aonyx_skills::merge_skills(builtins, user_skills),
        Err(e) => {
            eprintln!(
                "aonyx: could not load user skills from {}: {e}",
                user_dir.display()
            );
            builtins
        }
    }
}

/// `aonyx skills list` — print the active skill catalogue: built-ins
/// plus any user skills under `~/.aonyx/skills/`, tagging each by origin
/// and showing a one-line trigger summary (Phase QQ).
fn handle_skills_list() {
    let builtins: std::collections::HashSet<String> = aonyx_skills::builtin_skills()
        .into_iter()
        .map(|s| s.id)
        .collect();
    let skills = load_all_skills();
    if skills.is_empty() {
        println!("(no skills found)");
        return;
    }
    println!("{} skill(s):", skills.len());
    for s in &skills {
        let origin = if builtins.contains(&s.id) {
            "builtin"
        } else {
            "user"
        };
        let state = if s.enabled { "on" } else { "off" };
        println!("  • {} [{origin}, {state}]  {}", s.id, s.name);
        let t = &s.trigger;
        let mut hints = Vec::new();
        if t.always_on {
            hints.push("always-on".to_string());
        }
        if t.manual {
            hints.push("manual".to_string());
        }
        if !t.keywords.is_empty() {
            hints.push(format!("keywords: {}", t.keywords.join(", ")));
        }
        if let Some(p) = &t.project_matches {
            hints.push(format!("project ~ /{p}/"));
        }
        if !hints.is_empty() {
            println!("      {}", hints.join(" · "));
        }
    }
}

fn handle_config(action: ConfigAction) -> anyhow::Result<()> {
    let path = Config::config_path()?;
    match action {
        ConfigAction::Path => {
            println!("{}", path.display());
        }
        ConfigAction::Show => {
            let cfg = Config::load_or_init()?;
            println!("# {}\n", path.display());
            println!("{}", toml::to_string_pretty(&cfg)?);
        }
    }
    Ok(())
}

async fn handle_memory(action: MemoryAction) -> anyhow::Result<()> {
    use aonyx_core::MemoryStore;
    use aonyx_memory::{ChunksStore, DiaryStore, KgStore};

    let project_root = std::env::current_dir()?;
    let palace_dir = Palace::default_project_dir(&project_root);
    let palace = Palace::open(&palace_dir)?;
    let slug = project_slug(&project_root);

    match action {
        MemoryAction::Stats => {
            let entities = palace.kg.count_entities().await?;
            let diary_entries = palace.diary.count(&slug).await?;
            let chunks = palace.chunks.count(None).await?;
            println!("palace dir: {}", palace_dir.display());
            println!("project:    {slug}");
            println!("kg entities:    {entities}");
            println!("diary entries:  {diary_entries}");
            println!("chunks (FTS5):  {chunks}");
        }
        MemoryAction::Search { query } => {
            let hits = palace.hybrid_search(&query, 10).await?;
            if hits.is_empty() {
                println!("(no matches — ingest some chunks first; vector search lands in V1.1)");
            } else {
                for (idx, (content, score)) in hits.iter().enumerate() {
                    let preview: String = content.chars().take(160).collect();
                    let ellipsis = if content.chars().count() > 160 {
                        "…"
                    } else {
                        ""
                    };
                    println!("{:>2}. [score {:.3}] {preview}{ellipsis}", idx + 1, score);
                }
            }
        }
    }
    Ok(())
}
