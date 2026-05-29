//! Configuration loading / persisting for the `aonyx` binary.
//!
//! V1 layout:
//!
//! ```text
//! ~/.aonyx/
//! ├── config.toml      # provider, model, defaults
//! └── sessions.db      # (P2) cross-project session FTS5 store
//! ```
//!
//! Per-project palace lives at `<project_root>/.aonyx/{kg.db,diary.db}` — see
//! [`aonyx_memory::Palace::default_project_dir`].

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DEFAULT_MODEL: &str = "claude-sonnet-4-5-20250929";
const DEFAULT_SYSTEM_PROMPT: &str = "You are Aonyx Agent — the agent with a real memory palace. Be concise. Cite sources when you recall facts. Confirm scope before destructive actions.";

/// User-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Provider id — one of: `"anthropic"`, `"openai"`, `"openrouter"`,
    /// `"ollama"`, `"lm-studio"`.
    pub provider: String,
    /// Model identifier as understood by the provider.
    pub model: String,
    /// Anthropic API key. `null` falls back to `ANTHROPIC_API_KEY` env var.
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
    /// OpenAI API key. `null` falls back to `OPENAI_API_KEY` env var.
    #[serde(default)]
    pub openai_api_key: Option<String>,
    /// OpenRouter API key. `null` falls back to `OPENROUTER_API_KEY` env var.
    #[serde(default)]
    pub openrouter_api_key: Option<String>,
    /// Override OpenAI base URL (defaults to `https://api.openai.com`).
    #[serde(default)]
    pub openai_base_url: Option<String>,
    /// LM Studio base URL (defaults to `http://localhost:1234`).
    #[serde(default)]
    pub lm_studio_base_url: Option<String>,
    /// Ollama base URL (defaults to `http://localhost:11434`).
    #[serde(default)]
    pub ollama_base_url: Option<String>,
    /// Path to the `claude` binary (defaults to `claude` on PATH).
    #[serde(default)]
    pub claude_code_binary: Option<String>,
    /// Extra arguments forwarded to every `claude` invocation
    /// (e.g. `["--max-turns", "5"]`).
    #[serde(default)]
    pub claude_code_extra_args: Vec<String>,
    /// OpenRouter `HTTP-Referer` attribution header.
    #[serde(default)]
    pub openrouter_referer: Option<String>,
    /// OpenRouter `X-Title` attribution header.
    #[serde(default)]
    pub openrouter_title: Option<String>,
    /// Default system prompt injected at session start.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Maximum agent-loop iterations per user turn.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// TUI theme name (`default`, `catppuccin`, `dracula`, `gruvbox`).
    #[serde(default)]
    pub theme: Option<String>,
    /// Show reasoning blocks (when a provider emits them) under each turn.
    #[serde(default)]
    pub show_thinking: bool,
    /// Emit a desktop notification when a turn finishes or errors out.
    #[serde(default)]
    pub desktop_notifications: bool,
    /// Auto-compact the conversation once its estimated token count
    /// crosses [`Self::auto_compact_threshold`]. Off by default — when
    /// off, the TUI only nudges you to run `/compact` (Phase BB).
    #[serde(default)]
    pub auto_compact: bool,
    /// Estimated-token threshold that arms auto-compaction (and the
    /// manual-compaction nudge). Defaults to 24000.
    #[serde(default = "default_compact_threshold")]
    pub auto_compact_threshold: u64,
    /// External MCP servers to connect at startup; their tools join the
    /// registry (Phase GG).
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}

/// An MCP server declaration. Either **stdio** (set `command`, Phase GG)
/// or **HTTP** (set `url`, Phase II) — `url` wins when both are present.
///
/// ```toml
/// # stdio
/// [[mcp_servers]]
/// name = "brave"
/// command = "npx"
/// args = ["-y", "@modelcontextprotocol/server-brave-search"]
///
/// # HTTP (Streamable HTTP)
/// [[mcp_servers]]
/// name = "remote"
/// url = "https://mcp.example.com/v1"
/// bearer_token = "sk-…"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Friendly name — namespaces the server's tools (`<name>__<tool>`).
    pub name: String,
    /// Executable to spawn for the stdio transport. Ignored when `url`
    /// is set.
    #[serde(default)]
    pub command: Option<String>,
    /// Arguments passed to the stdio executable.
    #[serde(default)]
    pub args: Vec<String>,
    /// HTTP endpoint for the Streamable-HTTP transport (Phase II).
    #[serde(default)]
    pub url: Option<String>,
    /// Optional bearer token for the HTTP transport.
    #[serde(default)]
    pub bearer_token: Option<String>,
}

fn default_compact_threshold() -> u64 {
    24_000
}

fn default_max_iterations() -> usize {
    10
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: DEFAULT_MODEL.to_string(),
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            openrouter_api_key: std::env::var("OPENROUTER_API_KEY").ok(),
            openai_base_url: None,
            lm_studio_base_url: None,
            ollama_base_url: None,
            claude_code_binary: None,
            claude_code_extra_args: Vec::new(),
            openrouter_referer: None,
            openrouter_title: None,
            system_prompt: Some(DEFAULT_SYSTEM_PROMPT.to_string()),
            max_iterations: default_max_iterations(),
            theme: None,
            show_thinking: false,
            desktop_notifications: false,
            auto_compact: false,
            auto_compact_threshold: default_compact_threshold(),
            mcp_servers: Vec::new(),
        }
    }
}

impl Config {
    /// `~/.aonyx/`.
    pub fn config_dir() -> anyhow::Result<PathBuf> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not resolve home directory"))?;
        Ok(home.join(".aonyx"))
    }

    /// `~/.aonyx/config.toml`.
    pub fn config_path() -> anyhow::Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    /// Read the config, creating a default file when none exists.
    pub fn load_or_init() -> anyhow::Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            std::fs::create_dir_all(Self::config_dir()?)?;
            let default = Self::default();
            std::fs::write(&path, toml::to_string_pretty(&default)?)?;
            eprintln!("aonyx: created {}", path.display());
            return Ok(default);
        }
        let raw = std::fs::read_to_string(&path)?;
        let mut config: Config = toml::from_str(&raw)?;
        if config.anthropic_api_key.is_none() {
            config.anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        }
        if config.openai_api_key.is_none() {
            config.openai_api_key = std::env::var("OPENAI_API_KEY").ok();
        }
        if config.openrouter_api_key.is_none() {
            config.openrouter_api_key = std::env::var("OPENROUTER_API_KEY").ok();
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_anthropic() {
        let c = Config::default();
        assert_eq!(c.provider, "anthropic");
        assert_eq!(c.max_iterations, 10);
    }

    #[test]
    fn toml_round_trip_preserves_fields() {
        let original = Config {
            provider: "ollama".into(),
            model: "llama3.1:8b".into(),
            anthropic_api_key: Some("sk-test".into()),
            openai_api_key: None,
            openrouter_api_key: None,
            openai_base_url: None,
            lm_studio_base_url: None,
            ollama_base_url: Some("http://localhost:9999".into()),
            claude_code_binary: None,
            claude_code_extra_args: Vec::new(),
            openrouter_referer: None,
            openrouter_title: None,
            system_prompt: Some("be quiet".into()),
            max_iterations: 5,
            theme: Some("dracula".into()),
            show_thinking: true,
            desktop_notifications: false,
            auto_compact: true,
            auto_compact_threshold: 12_000,
            mcp_servers: vec![McpServerConfig {
                name: "demo".into(),
                command: Some("echo".into()),
                args: vec!["hi".into()],
                url: None,
                bearer_token: None,
            }],
        };
        let s = toml::to_string(&original).unwrap();
        let parsed: Config = toml::from_str(&s).unwrap();
        assert_eq!(parsed.provider, original.provider);
        assert_eq!(parsed.model, original.model);
        assert_eq!(parsed.max_iterations, original.max_iterations);
        assert_eq!(parsed.system_prompt.as_deref(), Some("be quiet"));
        assert_eq!(
            parsed.ollama_base_url.as_deref(),
            Some("http://localhost:9999")
        );
        assert!(parsed.auto_compact);
        assert_eq!(parsed.auto_compact_threshold, 12_000);
    }

    #[test]
    fn missing_compact_fields_use_defaults() {
        let raw = r#"
            provider = "anthropic"
            model = "claude-sonnet"
        "#;
        let parsed: Config = toml::from_str(raw).unwrap();
        assert!(!parsed.auto_compact);
        assert_eq!(parsed.auto_compact_threshold, 24_000);
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let raw = r#"
            provider = "anthropic"
            model = "claude-sonnet"
        "#;
        let parsed: Config = toml::from_str(raw).unwrap();
        assert_eq!(parsed.max_iterations, 10);
        assert!(parsed.system_prompt.is_none());
    }
}
