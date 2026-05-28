//! `aonyx` — the Aonyx Agent command-line binary.
//!
//! ```text
//! aonyx                  open an interactive session in the current dir
//! aonyx new <path>       start a new session scoped to <path>
//! aonyx resume [id]      resume a previous session                 (V1.1)
//! aonyx config <subcmd>  manage configuration                       (V1.1)
//! aonyx memory <subcmd>  inspect / search / export / import         (V1.1)
//! aonyx skills <subcmd>  list / install / enable / disable          (V1.1)
//! aonyx mcp <subcmd>     run the MCP server or connect to a remote  (V1.1)
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
use aonyx_memory::Palace;
use clap::{Parser, Subcommand};

mod config;
mod session;

use config::Config;
use session::InteractiveSession;

/// Aonyx Agent — the agent with a real memory palace.
#[derive(Debug, Parser)]
#[command(name = "aonyx", version, about, long_about = None)]
struct Cli {
    /// Verbose logging (`debug` level).
    #[arg(short, long, global = true)]
    verbose: bool,

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
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        None => start_interactive(None).await,
        Some(Command::New { path }) => start_interactive(path).await,
        Some(Command::Resume { id }) => {
            println!("aonyx resume {id:?} — coming in V1.1");
            Ok(())
        }
        Some(Command::Config { action }) => handle_config(action),
        Some(Command::Memory { action }) => handle_memory(action).await,
        Some(Command::Skills { action }) => {
            match action {
                SkillsAction::List => {
                    println!("aonyx skills list — coming in V1.1");
                }
            }
            Ok(())
        }
        Some(Command::Mcp { action }) => {
            match action {
                McpAction::Serve { port } => {
                    println!("aonyx mcp serve port={port:?} — coming in V1.1");
                }
            }
            Ok(())
        }
    }
}

fn init_tracing(verbose: bool) {
    let level = if verbose { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn start_interactive(project_path: Option<PathBuf>) -> anyhow::Result<()> {
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

    let skills = aonyx_skills::builtin_skills();
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
    stored
        .clone()
        .or_else(|| std::env::var(env_var).ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{config_field} missing — set it in ~/.aonyx/config.toml or export {env_var}"
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
