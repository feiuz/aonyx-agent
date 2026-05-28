//! Interactive REPL session: read user input, drive [`AgentRunner`] in
//! streaming mode, render text deltas + tool activity in real time, persist
//! a diary trail in the project palace.

use std::sync::Arc;

use aonyx_agent::{AgentRunner, ApprovalPolicy, TurnEvent};
use aonyx_core::{LlmProvider, MemoryStore, Message, Role, SafetyClass};
use aonyx_memory::Palace;
use aonyx_skills::Skill;
use aonyx_tools::ToolRegistry;
use termimad::MadSkin;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// Per-turn rendering state. Reset between turns.
struct DisplayState {
    /// Raw text accumulated during streaming, used to re-render Markdown
    /// at `AssistantMessageEnd`.
    assistant_buffer: String,
    /// Newline count in `assistant_buffer` — drives the cursor rewind on
    /// re-render.
    lines_during_stream: u32,
    /// Termimad skin (style for headings, code, lists, …).
    skin: MadSkin,
}

impl DisplayState {
    fn new() -> Self {
        Self {
            assistant_buffer: String::new(),
            lines_during_stream: 0,
            skin: MadSkin::default_dark(),
        }
    }

    fn reset(&mut self) {
        self.assistant_buffer.clear();
        self.lines_during_stream = 0;
    }
}

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
            stdout.write_all(b"\x1b[1myou>\x1b[0m ").await?;
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
            stdout.write_all(b"\n\x1b[1maonyx>\x1b[0m ").await?;
            stdout.flush().await?;

            let result = self.run_turn(&mut stdout).await;
            match result {
                Ok(()) => {
                    self.persist_turn(trimmed).await;
                }
                Err(e) => {
                    let msg = format!("\n\x1b[31m[error]\x1b[0m {e}\n");
                    stdout.write_all(msg.as_bytes()).await?;
                }
            }
            stdout.write_all(b"\n").await?;
        }

        Ok(())
    }

    async fn run_turn<W>(&mut self, out: &mut W) -> anyhow::Result<()>
    where
        W: AsyncWriteExt + Unpin,
    {
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(128);
        let messages_in = self.messages.clone();
        let mut state = DisplayState::new();

        let display = async {
            while let Some(event) = rx.recv().await {
                if let Err(e) = display_event(out, &mut state, &event).await {
                    return Err::<(), anyhow::Error>(anyhow::Error::from(e));
                }
            }
            Ok::<(), anyhow::Error>(())
        };
        let drive = self.runner.run_streaming(messages_in, tx);

        let (turn_res, display_res) = tokio::join!(drive, display);
        display_res?;
        let turn = turn_res?;
        self.messages = turn.messages;
        Ok(())
    }

    async fn handle_slash<W: AsyncWriteExt + Unpin>(
        &mut self,
        cmd: SlashCommand,
        out: &mut W,
    ) -> anyhow::Result<bool> {
        match cmd {
            SlashCommand::Quit => Ok(false),
            SlashCommand::Clear => {
                self.reset_history();
                out.write_all(b"\x1b[90m(history cleared)\x1b[0m\n").await?;
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

async fn display_event<W>(
    out: &mut W,
    state: &mut DisplayState,
    event: &TurnEvent,
) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    match event {
        TurnEvent::AssistantDelta(text) => {
            // Stream raw text so the user sees tokens arriving in real time…
            out.write_all(text.as_bytes()).await?;
            out.flush().await?;
            // …and remember everything so we can re-render Markdown at the end.
            state.assistant_buffer.push_str(text);
            state.lines_during_stream += text.matches('\n').count() as u32;
        }
        TurnEvent::AssistantMessageEnd => {
            if state.assistant_buffer.is_empty() {
                out.write_all(b"\n").await?;
                out.flush().await?;
            } else {
                // Rewind the cursor over every streamed row, clear, and re-print
                // the rendered Markdown above the original "aonyx>" label.
                if state.lines_during_stream > 0 {
                    let up = format!("\x1b[{}A", state.lines_during_stream);
                    out.write_all(up.as_bytes()).await?;
                }
                out.write_all(b"\r\x1b[J").await?;
                out.write_all(b"\x1b[1maonyx>\x1b[0m ").await?;

                let rendered = state.skin.term_text(&state.assistant_buffer).to_string();
                out.write_all(rendered.as_bytes()).await?;
                if !rendered.ends_with('\n') {
                    out.write_all(b"\n").await?;
                }
                out.flush().await?;
                state.reset();
            }
        }
        TurnEvent::IterationStart(n) if *n > 1 => {
            let line = format!("\x1b[90m[iter {n}]\x1b[0m\n");
            out.write_all(line.as_bytes()).await?;
            out.flush().await?;
        }
        TurnEvent::ToolStart { name, args, class } => {
            let dot = match class {
                SafetyClass::Safe => "\x1b[36m●\x1b[0m",
                SafetyClass::Caution => "\x1b[33m●\x1b[0m",
                SafetyClass::Destructive => "\x1b[31m●\x1b[0m",
            };
            let preview = abbreviate_value(args, 80);
            let line = format!("{dot} \x1b[36m{name}\x1b[0m\x1b[90m({preview})\x1b[0m\n");
            out.write_all(line.as_bytes()).await?;
            out.flush().await?;
        }
        TurnEvent::ToolEnd { name, ok, summary } => {
            let symbol = if *ok {
                "\x1b[32m  \u{21B3}\x1b[0m"
            } else {
                "\x1b[31m  \u{21B3}\x1b[0m"
            };
            let line = format!("{symbol} \x1b[90m{name}: {summary}\x1b[0m\n");
            out.write_all(line.as_bytes()).await?;
            out.flush().await?;
        }
        TurnEvent::ToolRejected { name, class } => {
            let line = format!("  \x1b[31mrejected:\x1b[0m \x1b[90m{name} ({class:?})\x1b[0m\n");
            out.write_all(line.as_bytes()).await?;
            out.flush().await?;
        }
        TurnEvent::Done {
            max_iterations_hit: true,
            iterations,
        } => {
            let line = format!("\x1b[33m(loop hit max_iterations = {iterations})\x1b[0m\n");
            out.write_all(line.as_bytes()).await?;
            out.flush().await?;
        }
        _ => {}
    }
    Ok(())
}

fn abbreviate_value(value: &serde_json::Value, max_chars: usize) -> String {
    let mut s = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    s = s.replace('\n', " ");
    if s.chars().count() > max_chars {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    } else {
        s
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

    #[test]
    fn abbreviate_value_truncates_long_strings() {
        let v = serde_json::Value::String("x".repeat(200));
        let s = abbreviate_value(&v, 50);
        assert!(s.chars().count() <= 51, "got: {s}");
        assert!(s.ends_with('…'));
    }

    #[test]
    fn abbreviate_value_keeps_short_strings_intact() {
        let v = serde_json::json!({ "path": "a.txt" });
        let s = abbreviate_value(&v, 80);
        assert!(s.contains("a.txt"));
        assert!(!s.contains('…'));
    }
}
