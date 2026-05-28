//! `aonyx` — the Aonyx Agent command-line binary.
//!
//! ```text
//! aonyx                  open an interactive session (current dir)
//! aonyx new <path>       start a new session scoped to <path>
//! aonyx resume [id]      resume a previous session
//! aonyx config <subcmd>  manage configuration (provider, model, keys)
//! aonyx memory <subcmd>  inspect / search / export / import the palace
//! aonyx skills <subcmd>  list / install / enable / disable skills
//! aonyx mcp <subcmd>     run the MCP server or connect to a remote one
//! ```

#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};

/// Aonyx Agent — the agent with a real memory palace.
#[derive(Debug, Parser)]
#[command(name = "aonyx", version, about, long_about = None)]
struct Cli {
    /// Verbose logging.
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
        path: Option<std::path::PathBuf>,
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
    /// List current configuration.
    List,
    /// Read a single key.
    Get { key: String },
    /// Set a key.
    Set { key: String, value: String },
}

#[derive(Debug, Subcommand)]
enum MemoryAction {
    /// Show counts and health for the palace.
    Stats,
    /// Hybrid-search across chunks.
    Search { query: String },
    /// Export the palace to a portable archive.
    Export { out: std::path::PathBuf },
    /// Import a palace archive.
    Import { input: std::path::PathBuf },
}

#[derive(Debug, Subcommand)]
enum SkillsAction {
    /// List known skills.
    List,
    /// Install a skill from a URL or path.
    Install { source: String },
    /// Enable a skill by id.
    Enable { id: String },
    /// Disable a skill by id.
    Disable { id: String },
}

#[derive(Debug, Subcommand)]
enum McpAction {
    /// Serve the Aonyx MCP server on stdio or a port.
    Serve {
        /// TCP port; if omitted, serve over stdio.
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Connect to a remote MCP server.
    Connect { url: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        None => interactive_session().await,
        Some(Command::New { path }) => {
            println!("aonyx new {:?} — coming in V1", path);
            Ok(())
        }
        Some(Command::Resume { id }) => {
            println!("aonyx resume {:?} — coming in V1", id);
            Ok(())
        }
        Some(Command::Config { action }) => {
            println!("aonyx config {:?} — coming in V1", action);
            Ok(())
        }
        Some(Command::Memory { action }) => {
            println!("aonyx memory {:?} — coming in V1", action);
            Ok(())
        }
        Some(Command::Skills { action }) => {
            println!("aonyx skills {:?} — coming in V1", action);
            Ok(())
        }
        Some(Command::Mcp { action }) => {
            println!("aonyx mcp {:?} — coming in V1", action);
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

async fn interactive_session() -> anyhow::Result<()> {
    println!("🦦  Aonyx Agent — pre-alpha");
    println!();
    println!("This is a scaffold. The interactive loop lands in Vague 1.");
    println!("See `.bmad/prd.md` for the MVP plan.");
    Ok(())
}
