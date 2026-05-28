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
            openrouter_referer: None,
            openrouter_title: None,
            system_prompt: Some(DEFAULT_SYSTEM_PROMPT.to_string()),
            max_iterations: default_max_iterations(),
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
            openrouter_referer: None,
            openrouter_title: None,
            system_prompt: Some("be quiet".into()),
            max_iterations: 5,
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
