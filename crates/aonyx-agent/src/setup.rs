//! `aonyx setup` — the interactive configuration wizard.
//!
//! Walks the user through choosing an LLM provider, entering credentials
//! (stored in the OS keyring when one is available), picking a model, and
//! verifying everything with a live connection test — then writes
//! `~/.aonyx/config.toml`. Secrets go to the keyring, never the file,
//! unless the user explicitly opts into a plaintext fallback.

use std::sync::Arc;

use aonyx_core::{ChatRequest, LlmProvider, Message, Role};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password, Select};
use futures::StreamExt;

use crate::config::Config;
use crate::secrets;

/// Provider menu: `(id, human label, needs an API key)`.
const PROVIDERS: &[(&str, &str, bool)] = &[
    ("anthropic", "Anthropic (Claude)", true),
    ("openai", "OpenAI", true),
    ("openrouter", "OpenRouter", true),
    ("ollama", "Ollama (local)", false),
    ("lm-studio", "LM Studio (local)", false),
    (
        "claude-code",
        "Claude Code (no key — uses the `claude` CLI)",
        false,
    ),
];

/// Entry point for `aonyx setup` / `aonyx setup provider`.
pub async fn run_provider_wizard() -> anyhow::Result<()> {
    let theme = ColorfulTheme::default();
    println!("aonyx setup — configure your LLM provider\n");

    // Operate on the on-disk config (no env merge) so we never round-trip
    // an env-sourced key back into the file.
    let mut config = Config::load_raw()?;

    let labels: Vec<&str> = PROVIDERS.iter().map(|p| p.1).collect();
    let default_idx = PROVIDERS
        .iter()
        .position(|p| p.0 == config.provider)
        .unwrap_or(0);
    let idx = Select::with_theme(&theme)
        .with_prompt("Provider")
        .items(&labels)
        .default(default_idx)
        .interact()?;
    let (provider, _label, needs_key) = PROVIDERS[idx];
    config.provider = provider.to_string();

    // Credentials (key-based providers only).
    if needs_key {
        if let Some((field, env_var)) = key_slots(provider) {
            let key: String = Password::with_theme(&theme)
                .with_prompt(format!(
                    "{env_var} (input hidden — leave empty to use $env)"
                ))
                .allow_empty_password(true)
                .interact()?;
            if key.trim().is_empty() {
                println!("  · no key entered — will read ${env_var} at runtime");
                clear_key_field(&mut config, field);
            } else {
                store_key(&mut config, field, key.trim(), &theme)?;
            }
        }
    }

    // Endpoint / binary for the remaining providers.
    match provider {
        "ollama" => {
            config.ollama_base_url = Some(prompt_default(
                &theme,
                "Ollama base URL",
                config
                    .ollama_base_url
                    .clone()
                    .unwrap_or_else(|| aonyx_llm::OLLAMA_DEFAULT_BASE_URL.to_string()),
            )?);
        }
        "lm-studio" => {
            config.lm_studio_base_url = Some(prompt_default(
                &theme,
                "LM Studio base URL",
                config.lm_studio_base_url.clone().unwrap_or_else(|| {
                    aonyx_llm::lm_studio::LM_STUDIO_DEFAULT_BASE_URL.to_string()
                }),
            )?);
        }
        "claude-code" => {
            config.claude_code_binary = Some(prompt_default(
                &theme,
                "Path to the `claude` binary",
                config
                    .claude_code_binary
                    .clone()
                    .unwrap_or_else(|| aonyx_llm::CLAUDE_DEFAULT_BIN.to_string()),
            )?);
        }
        _ => {}
    }

    // Model.
    config.model = prompt_default(&theme, "Model", default_model(provider, &config.model))?;

    // Live connection test (skip for claude-code — that shells out to the
    // `claude` CLI, which we don't want to spawn just to ping).
    if provider != "claude-code"
        && Confirm::with_theme(&theme)
            .with_prompt("Test the connection now?")
            .default(true)
            .interact()?
    {
        match crate::build_provider(&config) {
            Ok(p) => match test_connection(&p, &config.model).await {
                Ok(()) => println!("  ✓ connection OK"),
                Err(e) => {
                    println!("  ✗ connection failed: {e}");
                    if !Confirm::with_theme(&theme)
                        .with_prompt("Save the config anyway?")
                        .default(true)
                        .interact()?
                    {
                        println!("aborted — nothing written.");
                        return Ok(());
                    }
                }
            },
            Err(e) => println!("  ✗ could not build provider: {e} (will retry at runtime)"),
        }
    }

    config.save()?;
    println!("\n✓ wrote {}", Config::config_path()?.display());
    println!("  run `aonyx` to start a session.");
    Ok(())
}

/// Suggested default model per provider, falling back to whatever is
/// already configured for an unknown id.
fn default_model(provider: &str, current: &str) -> String {
    match provider {
        "anthropic" => "claude-sonnet-4-5-20250929".to_string(),
        "openai" => "gpt-4o".to_string(),
        "openrouter" => "anthropic/claude-3.5-sonnet".to_string(),
        "ollama" => "llama3.1:8b".to_string(),
        "lm-studio" => "local-model".to_string(),
        _ => current.to_string(),
    }
}

/// `(keyring key / config field, environment variable)` for a key-based
/// provider. The keyring key intentionally matches the `config.toml`
/// field name so the two storages share one identifier.
fn key_slots(provider: &str) -> Option<(&'static str, &'static str)> {
    match provider {
        "anthropic" => Some(("anthropic_api_key", "ANTHROPIC_API_KEY")),
        "openai" => Some(("openai_api_key", "OPENAI_API_KEY")),
        "openrouter" => Some(("openrouter_api_key", "OPENROUTER_API_KEY")),
        _ => None,
    }
}

/// Prompt for a free-text value with a pre-filled default.
fn prompt_default(theme: &ColorfulTheme, prompt: &str, default: String) -> anyhow::Result<String> {
    Ok(Input::<String>::with_theme(theme)
        .with_prompt(prompt)
        .default(default)
        .interact_text()?)
}

/// Store an API key in the keyring; on failure, offer a plaintext
/// fallback in `config.toml` or skip (leaving the env var as the source).
fn store_key(
    config: &mut Config,
    field: &str,
    key: &str,
    theme: &ColorfulTheme,
) -> anyhow::Result<()> {
    match secrets::set(field, key) {
        Ok(()) => {
            println!("  ✓ stored in the OS keyring");
            // Make sure no stale plaintext copy survives in the file.
            clear_key_field(config, field);
        }
        Err(e) => {
            println!("  ⚠ keyring unavailable ({e})");
            let plain = Confirm::with_theme(theme)
                .with_prompt("Store the key in ~/.aonyx/config.toml as plaintext instead?")
                .default(false)
                .interact()?;
            if plain {
                set_key_field(config, field, key);
                println!("  ✓ stored in config.toml (plaintext)");
            } else {
                clear_key_field(config, field);
                println!(
                    "  · skipped — export ${} to use this provider",
                    key_slots_env(field)
                );
            }
        }
    }
    Ok(())
}

fn set_key_field(c: &mut Config, field: &str, key: &str) {
    match field {
        "anthropic_api_key" => c.anthropic_api_key = Some(key.to_string()),
        "openai_api_key" => c.openai_api_key = Some(key.to_string()),
        "openrouter_api_key" => c.openrouter_api_key = Some(key.to_string()),
        _ => {}
    }
}

fn clear_key_field(c: &mut Config, field: &str) {
    match field {
        "anthropic_api_key" => c.anthropic_api_key = None,
        "openai_api_key" => c.openai_api_key = None,
        "openrouter_api_key" => c.openrouter_api_key = None,
        _ => {}
    }
}

fn key_slots_env(field: &str) -> &'static str {
    match field {
        "anthropic_api_key" => "ANTHROPIC_API_KEY",
        "openai_api_key" => "OPENAI_API_KEY",
        "openrouter_api_key" => "OPENROUTER_API_KEY",
        _ => "",
    }
}

/// Fire a one-shot, tiny completion and pull the first stream frame to
/// confirm the endpoint and credentials actually work.
async fn test_connection(provider: &Arc<dyn LlmProvider>, model: &str) -> anyhow::Result<()> {
    let req = ChatRequest {
        model: model.to_string(),
        messages: vec![Message::new(Role::User, "ping")],
        tools: Vec::new(),
        temperature: None,
        max_tokens: Some(16),
    };
    let mut stream = provider
        .chat_stream(req)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    match stream.next().await {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(anyhow::anyhow!("{e}")),
        // An empty-but-clean stream still proves the endpoint answered.
        None => Ok(()),
    }
}

/// Entry point for `aonyx setup telegram` — store the bot token in the
/// keyring and the allowed-chat list in `config.toml`. Always available
/// (writing config is light); actually running the bot needs the
/// `telegram` build feature.
pub async fn run_telegram_wizard() -> anyhow::Result<()> {
    let theme = ColorfulTheme::default();
    println!("aonyx setup telegram — configure the Telegram bot\n");
    let mut config = Config::load_raw()?;

    let token: String = Password::with_theme(&theme)
        .with_prompt("Bot token from @BotFather (hidden — empty to keep current / use $env)")
        .allow_empty_password(true)
        .interact()?;
    if token.trim().is_empty() {
        println!("  · no token entered — will read $TELEGRAM_BOT_TOKEN at runtime");
    } else {
        match secrets::set("telegram_bot_token", token.trim()) {
            Ok(()) => println!("  ✓ token stored in the OS keyring"),
            Err(e) => println!("  ⚠ keyring unavailable ({e}) — export TELEGRAM_BOT_TOKEN instead"),
        }
    }

    let current = config
        .telegram_allowed_chats
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let chats: String = Input::<String>::with_theme(&theme)
        .with_prompt("Allowed chat ids, comma-separated (empty = allow everyone)")
        .allow_empty(true)
        .default(current)
        .interact_text()?;
    config.telegram_allowed_chats = parse_chat_ids(&chats);

    config.save()?;
    println!("\n✓ wrote {}", Config::config_path()?.display());
    if config.telegram_allowed_chats.is_empty() {
        println!("  ⚠ no allow-list — the bot will answer ANY chat. Add ids to lock it down.");
    }
    if cfg!(feature = "telegram") {
        println!("  run `aonyx serve telegram` to start the bot.");
    } else {
        println!(
            "  this build lacks Telegram support — reinstall with \
             `--features telegram` to run the bot."
        );
    }
    Ok(())
}

/// Parse a comma-separated list of chat ids, dropping blanks / non-numbers.
fn parse_chat_ids(s: &str) -> Vec<i64> {
    s.split(',')
        .filter_map(|p| p.trim().parse::<i64>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_chat_ids;

    #[test]
    fn parses_and_skips_junk() {
        assert_eq!(parse_chat_ids("123, -456 ,abc,, 789"), vec![123, -456, 789]);
        assert!(parse_chat_ids("").is_empty());
    }
}
