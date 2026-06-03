//! Self-evolution (Phase BBB) — `aonyx reflect`.
//!
//! A bounded, deterministic take on the DSPy/GEPA idea: read the project's
//! own work diary, ask the model to distil the recurring patterns, user
//! preferences, and lessons from mistakes, and propose an improved system
//! prompt. The user sees a diff; `--apply` adopts it into `config.toml`.
//! The agent thus learns from its own history without an opaque online
//! optimiser.

use std::sync::Arc;

use aonyx_core::{ChatRequest, LlmProvider, Message, Role};
use aonyx_memory::{DiaryStore, Palace};
use futures::StreamExt;

use crate::config::Config;

/// Entry point for `aonyx reflect [--apply]`.
pub async fn run(apply: bool) -> anyhow::Result<()> {
    let config = Config::load_or_init()?;
    let provider = crate::build_provider(&config)?;
    let project_root = std::env::current_dir()?;
    let palace = Palace::open(Palace::default_project_dir(&project_root))?;
    let slug = crate::project_slug(&project_root);

    let entries = palace.diary.recent(&slug, 50).await.unwrap_or_default();
    if entries.is_empty() {
        println!("(no diary entries for '{slug}' yet — nothing to reflect on)");
        return Ok(());
    }

    let current = config.system_prompt.clone().unwrap_or_default();
    let (system, user) = build_prompt(&current, &entries);

    eprintln!("aonyx: reflecting over {} diary entries…", entries.len());
    let proposed = collect(&provider, &config.model, &system, &user)
        .await?
        .trim()
        .to_string();
    if proposed.is_empty() {
        anyhow::bail!("the model returned an empty proposal");
    }

    print_diff(&current, &proposed);

    if apply {
        use dialoguer::{theme::ColorfulTheme, Confirm};
        let ok = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Adopt this as the new system prompt?")
            .default(false)
            .interact()?;
        if ok {
            // Write against the on-disk config so we don't persist
            // env-sourced secrets (see Config::load_raw).
            let mut raw = Config::load_raw()?;
            raw.system_prompt = Some(proposed);
            raw.save()?;
            println!(
                "✓ updated system prompt in {}",
                Config::config_path()?.display()
            );
        } else {
            println!("not applied.");
        }
    } else {
        println!("\n(run `aonyx reflect --apply` to adopt it)");
    }
    Ok(())
}

/// Build the `(system, user)` prompt pair from the current prompt + diary.
fn build_prompt(current: &str, entries: &[aonyx_memory::DiaryEntry]) -> (String, String) {
    let digest = entries
        .iter()
        .map(|e| {
            let kind = e
                .kind
                .as_deref()
                .map(|k| format!("[{k}] "))
                .unwrap_or_default();
            format!("- {kind}{}", e.content.replace('\n', " "))
        })
        .collect::<Vec<_>>()
        .join("\n");

    let system = "You are refining your own standing instructions by reflecting on your work \
                  diary. Be honest and concrete; favour operational guidance over platitudes."
        .to_string();
    let user = format!(
        "CURRENT SYSTEM PROMPT:\n{current}\n\n\
         RECENT DIARY (newest first, {n} entries):\n{digest}\n\n\
         Propose an improved system prompt that bakes in the recurring patterns, the user's \
         preferences, and lessons from any mistakes visible above. Keep it concise and \
         operational. Output ONLY the new system prompt — no preamble, no markdown fences.",
        n = entries.len()
    );
    (system, user)
}

/// Collect a full (non-streaming-to-the-user) completion.
async fn collect(
    provider: &Arc<dyn LlmProvider>,
    model: &str,
    system: &str,
    user: &str,
) -> anyhow::Result<String> {
    let req = ChatRequest {
        model: model.to_string(),
        messages: vec![
            Message::new(Role::System, system),
            Message::new(Role::User, user),
        ],
        tools: Vec::new(),
        temperature: Some(0.3),
        max_tokens: Some(2048),
    };
    let mut stream = provider
        .chat_stream(req)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        let c = chunk.map_err(|e| anyhow::anyhow!("{e}"))?;
        out.push_str(&c.delta_text);
        if c.finished {
            break;
        }
    }
    Ok(out)
}

fn print_diff(old: &str, new: &str) {
    use similar::{ChangeTag, TextDiff};
    println!("\n── proposed system prompt (diff vs current) ──");
    let diff = TextDiff::from_lines(old, new);
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        print!("{sign} {change}");
    }
    println!("──────────────────────────────────────────────");
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_memory::DiaryEntry;

    #[test]
    fn prompt_includes_current_and_diary() {
        let entries = vec![
            DiaryEntry::new("proj", "fixed the auth bug").with_kind("decision"),
            DiaryEntry::new("proj", "user prefers concise replies"),
        ];
        let (system, user) = build_prompt("be helpful", &entries);
        assert!(system.contains("standing instructions"));
        assert!(user.contains("be helpful"));
        assert!(user.contains("[decision] fixed the auth bug"));
        assert!(user.contains("user prefers concise replies"));
        assert!(user.contains("2 entries"));
    }
}
