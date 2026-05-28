//! Interactive REPL session: read user input, drive [`AgentRunner`], stream
//! the response back, persist a diary trail in the project palace.

use std::sync::Arc;

use aonyx_agent::{AgentRunner, ApprovalPolicy};
use aonyx_core::{LlmProvider, MemoryStore, Message, Role};
use aonyx_memory::Palace;
use aonyx_skills::Skill;
use aonyx_tools::ToolRegistry;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Recognised slash commands inside an interactive session.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SlashCommand {
    /// Exit the session.
    Quit,
    /// Drop the conversation history (keeping the system prompt).
    Clear,
    /// Print the help blurb.
    Help,
}

impl SlashCommand {
    /// Parse a trimmed line that the user just typed.
    pub fn parse(line: &str) -> Option<Self> {
        match line {
            "/quit" | "/q" | "/exit" => Some(Self::Quit),
            "/clear" | "/reset" => Some(Self::Clear),
            "/help" | "/?" => Some(Self::Help),
            _ => None,
        }
    }
}

/// REPL driver.
pub struct InteractiveSession {
    runner: AgentRunner,
    palace: Palace,
    messages: Vec<Message>,
    project_slug: String,
}

impl InteractiveSession {
    /// Wire a runner from a provider + a freshly-opened palace.
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        palace: Palace,
        model: String,
        max_iterations: usize,
        system_prompt: Option<String>,
        project_slug: impl Into<String>,
        skills: Vec<Skill>,
    ) -> Self {
        let project = project_slug.into();
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), model)
            .with_max_iterations(max_iterations)
            .with_approval(ApprovalPolicy::DenyDestructive)
            .with_skills(skills)
            .with_project(&project);

        let mut messages = Vec::new();
        if let Some(prompt) = system_prompt {
            messages.push(Message::new(Role::System, prompt));
        }

        Self {
            runner,
            palace,
            messages,
            project_slug: project,
        }
    }

    /// Reset the message log, keeping the system prompt at index 0 if present.
    pub fn reset_history(&mut self) {
        let system = self
            .messages
            .first()
            .filter(|m| m.role == Role::System)
            .cloned();
        self.messages.clear();
        if let Some(s) = system {
            self.messages.push(s);
        }
    }

    /// Run the REPL loop against the current stdin / stdout.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut stdout = tokio::io::stdout();
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin).lines();

        stdout
            .write_all(b"\xf0\x9f\xa6\xa6  Aonyx Agent \xe2\x80\x94 interactive session\n")
            .await?;
        stdout
            .write_all(b"    /help for commands, /quit to exit\n\n")
            .await?;
        stdout.flush().await?;

        loop {
            stdout.write_all(b"you> ").await?;
            stdout.flush().await?;
            let Some(line) = reader.next_line().await? else {
                break;
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(cmd) = SlashCommand::parse(trimmed) {
                if !self.handle_slash(cmd, &mut stdout).await? {
                    break;
                }
                continue;
            }

            self.messages
                .push(Message::new(Role::User, trimmed.to_string()));
            stdout.write_all(b"\naonyx> ").await?;
            stdout.flush().await?;

            match self.runner.run(self.messages.clone()).await {
                Ok(result) => {
                    self.messages = result.messages;
                    if let Some(last) = self
                        .messages
                        .iter()
                        .rev()
                        .find(|m| m.role == Role::Assistant)
                    {
                        stdout.write_all(last.content.as_bytes()).await?;
                        stdout.write_all(b"\n").await?;
                    } else {
                        stdout
                            .write_all(b"(no assistant text - model ended on a tool call)\n")
                            .await?;
                    }
                    if result.max_iterations_hit {
                        stdout.write_all(b"(loop hit max_iterations)\n").await?;
                    }
                    self.persist_turn(trimmed).await;
                }
                Err(e) => {
                    let msg = format!("\n[error] {e}\n");
                    stdout.write_all(msg.as_bytes()).await?;
                }
            }
            stdout.write_all(b"\n").await?;
        }

        Ok(())
    }

    /// Returns `true` to continue the loop, `false` to exit.
    async fn handle_slash<W: AsyncWriteExt + Unpin>(
        &mut self,
        cmd: SlashCommand,
        out: &mut W,
    ) -> anyhow::Result<bool> {
        match cmd {
            SlashCommand::Quit => Ok(false),
            SlashCommand::Clear => {
                self.reset_history();
                out.write_all(b"(history cleared)\n").await?;
                Ok(true)
            }
            SlashCommand::Help => {
                out.write_all(
                    b"available commands:\n  /quit /q /exit   exit\n  /clear /reset    reset history (keep system prompt)\n  /help /?         this list\n",
                )
                .await?;
                Ok(true)
            }
        }
    }

    async fn persist_turn(&self, user_line: &str) {
        let summary = if user_line.len() > 160 {
            format!("turn: {}…", &user_line[..160])
        } else {
            format!("turn: {user_line}")
        };
        let _ = self.palace.diary_append(&self.project_slug, &summary).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_command_recognises_quit_aliases() {
        for s in ["/quit", "/q", "/exit"] {
            assert_eq!(SlashCommand::parse(s), Some(SlashCommand::Quit), "for {s}");
        }
    }

    #[test]
    fn slash_command_recognises_clear_aliases() {
        for s in ["/clear", "/reset"] {
            assert_eq!(SlashCommand::parse(s), Some(SlashCommand::Clear), "for {s}");
        }
    }

    #[test]
    fn slash_command_recognises_help_aliases() {
        for s in ["/help", "/?"] {
            assert_eq!(SlashCommand::parse(s), Some(SlashCommand::Help), "for {s}");
        }
    }

    #[test]
    fn slash_command_returns_none_for_chat_lines() {
        assert_eq!(SlashCommand::parse("hello world"), None);
        assert_eq!(SlashCommand::parse(""), None);
        assert_eq!(SlashCommand::parse("/unknown"), None);
    }
}
