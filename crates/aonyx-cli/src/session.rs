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
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SlashCommand {
    /// Exit the session.
    Quit,
    /// Drop the conversation history (keeping the system prompt). Alias of `New`.
    Clear,
    /// Same as `Clear` — sugar for "start a new conversation".
    New,
    /// Print the help blurb.
    Help,
    /// List configured providers and the current model.
    Models,
    /// List known sessions (V1: single-session, stub).
    Sessions,
    /// Export the current conversation to a Markdown file.
    Export(Option<String>),
    /// Toggle verbose tool-execution details.
    Details,
    /// Toggle reasoning-block visibility (stub in V1).
    Thinking,
    /// Open `$EDITOR` to compose a long message (wired in A3).
    Editor,
    /// Create a fresh `agent.yaml` in the current project.
    Init,
    /// Switch the TUI theme (`/themes <name>`); without args lists them.
    Themes(Option<String>),
    /// Toggle vim-style modal editing (F3). TUI-only — no-op in legacy.
    Vim,
}

impl SlashCommand {
    /// Parse a trimmed line that the user just typed.
    pub fn parse(line: &str) -> Option<Self> {
        let (head, rest) = match line.split_once(' ') {
            Some((h, r)) => (h, Some(r.trim())),
            None => (line, None),
        };
        match head {
            "/quit" | "/q" | "/exit" => Some(Self::Quit),
            "/clear" | "/reset" => Some(Self::Clear),
            "/new" | "/n" => Some(Self::New),
            "/help" | "/?" => Some(Self::Help),
            "/models" | "/m" => Some(Self::Models),
            "/sessions" | "/s" => Some(Self::Sessions),
            "/export" => Some(Self::Export(rest.map(str::to_string))),
            "/details" => Some(Self::Details),
            "/thinking" => Some(Self::Thinking),
            "/editor" | "/e" => Some(Self::Editor),
            "/init" => Some(Self::Init),
            "/themes" | "/theme" | "/t" => Some(Self::Themes(rest.map(str::to_string))),
            "/vim" => Some(Self::Vim),
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
    provider_name: String,
    model_name: String,
    turns: u32,
    show_tool_details: bool,
}

impl InteractiveSession {
    /// Wire a runner from a provider + a freshly-opened palace.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        palace: Palace,
        model: String,
        max_iterations: usize,
        system_prompt: Option<String>,
        project_slug: impl Into<String>,
        skills: Vec<Skill>,
        provider_name: impl Into<String>,
    ) -> Self {
        let project = project_slug.into();
        let model_name = model.clone();
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
            provider_name: provider_name.into(),
            model_name,
            turns: 0,
            show_tool_details: false,
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
                    self.turns += 1;
                    self.persist_turn(trimmed).await;
                }
                Err(e) => {
                    let msg = format!("\n\x1b[31m[error]\x1b[0m {e}\n");
                    stdout.write_all(msg.as_bytes()).await?;
                }
            }
            self.write_status_bar(&mut stdout).await?;
            stdout.write_all(b"\n").await?;
        }

        Ok(())
    }

    async fn write_status_bar<W: AsyncWriteExt + Unpin>(&self, out: &mut W) -> std::io::Result<()> {
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "?".to_string());
        let details = if self.show_tool_details {
            " · details:on"
        } else {
            ""
        };
        let bar = format!(
            "\x1b[90m\u{2500} {provider} \u{00b7} {model} \u{00b7} turn {turn} \u{00b7} cwd:{cwd}{details} \u{2500}\x1b[0m\n",
            provider = self.provider_name,
            model = self.model_name,
            turn = self.turns,
        );
        out.write_all(bar.as_bytes()).await?;
        out.flush().await
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
            SlashCommand::Clear | SlashCommand::New => {
                self.reset_history();
                self.turns = 0;
                out.write_all(b"\x1b[90m(history cleared)\x1b[0m\n").await?;
                Ok(true)
            }
            SlashCommand::Help => {
                out.write_all(HELP_BLURB).await?;
                Ok(true)
            }
            SlashCommand::Models => {
                let line = format!(
                    "\x1b[90mactive:\x1b[0m {} \u{00b7} {}\n\
                     \x1b[90mavailable providers:\x1b[0m anthropic \u{00b7} openai \u{00b7} openrouter \u{00b7} ollama \u{00b7} lm-studio \u{00b7} claude-code\n\
                     \x1b[90mswitch with:\x1b[0m edit ~/.aonyx/config.toml (live switch lands in V0.3)\n",
                    self.provider_name, self.model_name,
                );
                out.write_all(line.as_bytes()).await?;
                Ok(true)
            }
            SlashCommand::Sessions => {
                out.write_all(b"\x1b[90msingle-session mode (V0.4 ships multi-session storage with /resume /list)\x1b[0m\n").await?;
                Ok(true)
            }
            SlashCommand::Export(target) => {
                let path = export_path(target);
                match self.export_markdown(&path).await {
                    Ok(()) => {
                        let line = format!(
                            "\x1b[90mexported:\x1b[0m {} ({} messages)\n",
                            path.display(),
                            self.messages.len()
                        );
                        out.write_all(line.as_bytes()).await?;
                    }
                    Err(e) => {
                        let line = format!("\x1b[31mexport failed:\x1b[0m {e}\n");
                        out.write_all(line.as_bytes()).await?;
                    }
                }
                Ok(true)
            }
            SlashCommand::Details => {
                self.show_tool_details = !self.show_tool_details;
                let state = if self.show_tool_details { "on" } else { "off" };
                let line = format!("\x1b[90mtool details: {state}\x1b[0m\n");
                out.write_all(line.as_bytes()).await?;
                Ok(true)
            }
            SlashCommand::Thinking => {
                out.write_all(b"\x1b[90mreasoning visibility lands in V0.3 (Phase E)\x1b[0m\n")
                    .await?;
                Ok(true)
            }
            SlashCommand::Editor => {
                self.run_editor_turn(out).await?;
                Ok(true)
            }
            SlashCommand::Themes(_) => {
                out.write_all(b"\x1b[90m/themes runs in TUI mode (aonyx --tui)\x1b[0m\n")
                    .await?;
                Ok(true)
            }
            SlashCommand::Vim => {
                out.write_all(b"\x1b[90m/vim runs in TUI mode (aonyx --tui)\x1b[0m\n")
                    .await?;
                Ok(true)
            }
            SlashCommand::Init => {
                let path = std::path::PathBuf::from("agent.yaml");
                if path.exists() {
                    let line = format!(
                        "\x1b[33m{} already exists — leaving it alone\x1b[0m\n",
                        path.display()
                    );
                    out.write_all(line.as_bytes()).await?;
                } else {
                    let yaml = format!(
                        "# Aonyx Agent — per-project configuration\n\
                         persona: \"You are an Aonyx agent helping with {} .\"\n\
                         system_prompt: |\n  Be concise. Cite sources. Confirm destructive actions.\n\
                         preferred_provider: {}\n\
                         preferred_model: {}\n",
                        self.project_slug, self.provider_name, self.model_name
                    );
                    if let Err(e) = tokio::fs::write(&path, yaml).await {
                        let line = format!("\x1b[31minit failed:\x1b[0m {e}\n");
                        out.write_all(line.as_bytes()).await?;
                    } else {
                        let line = format!("\x1b[90mcreated:\x1b[0m {}\n", path.display());
                        out.write_all(line.as_bytes()).await?;
                    }
                }
                Ok(true)
            }
        }
    }

    async fn run_editor_turn<W: AsyncWriteExt + Unpin>(
        &mut self,
        out: &mut W,
    ) -> anyhow::Result<()> {
        let editor = resolve_editor();
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string();
        let tmp = std::env::temp_dir().join(format!("aonyx-msg-{stamp}.md"));
        let intro = "# Write your message below. Lines starting with `#?` are ignored.\n#? Save and exit your editor when you're done. Leave empty to cancel.\n\n";
        tokio::fs::write(&tmp, intro.as_bytes()).await?;

        let line = format!("\x1b[90mopening:\x1b[0m {} ({})\n", editor, tmp.display());
        out.write_all(line.as_bytes()).await?;
        out.flush().await?;

        let status = tokio::process::Command::new(&editor)
            .arg(&tmp)
            .status()
            .await;

        let raw = match status {
            Ok(s) if s.success() => tokio::fs::read_to_string(&tmp).await.unwrap_or_default(),
            Ok(s) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                let line = format!(
                    "\x1b[31meditor exited with status {} - skipping\x1b[0m\n",
                    s.code().unwrap_or(-1)
                );
                out.write_all(line.as_bytes()).await?;
                return Ok(());
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                let line = format!(
                    "\x1b[31mcould not launch {editor}:\x1b[0m {e}\n\
                     \x1b[90mhint:\x1b[0m set $EDITOR or $VISUAL to override the default.\n"
                );
                out.write_all(line.as_bytes()).await?;
                return Ok(());
            }
        };
        let _ = tokio::fs::remove_file(&tmp).await;

        let message = strip_editor_instructions(&raw);
        if message.trim().is_empty() {
            out.write_all(b"\x1b[90m(editor input empty - cancelled)\x1b[0m\n")
                .await?;
            return Ok(());
        }

        let preview = preview_first_line(&message);
        let line = format!("\x1b[90mfrom editor:\x1b[0m \x1b[1m{preview}\x1b[0m\n");
        out.write_all(line.as_bytes()).await?;

        self.messages
            .push(Message::new(Role::User, message.clone()));
        out.write_all(b"\n\x1b[1maonyx>\x1b[0m ").await?;
        out.flush().await?;
        match self.run_turn(out).await {
            Ok(()) => {
                self.turns += 1;
                self.persist_turn(&preview).await;
            }
            Err(e) => {
                let msg = format!("\n\x1b[31m[error]\x1b[0m {e}\n");
                out.write_all(msg.as_bytes()).await?;
            }
        }
        Ok(())
    }

    async fn export_markdown(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut out = String::new();
        out.push_str(&format!(
            "# Aonyx Agent session — {project}\n\n",
            project = self.project_slug
        ));
        out.push_str(&format!(
            "_provider: {} \u{00b7} model: {} \u{00b7} turns: {}_\n\n---\n\n",
            self.provider_name, self.model_name, self.turns,
        ));
        for m in &self.messages {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            out.push_str(&format!("### {role}\n\n{}\n\n", m.content));
        }
        tokio::fs::write(path, out).await
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

const HELP_BLURB: &[u8] = b"available commands:\n  \
/quit /q /exit       exit\n  \
/clear /reset /new   reset conversation (keep system prompt)\n  \
/help /?             this list\n  \
/models /m           current provider + model, list available\n  \
/sessions /s         list sessions (V0.4)\n  \
/export [path]       write the transcript to a Markdown file\n  \
/details             toggle verbose tool-execution rendering\n  \
/thinking            toggle reasoning visibility (V0.3)\n  \
/editor /e           compose a long message in $EDITOR (V0.3)\n  \
/init                drop an agent.yaml in the current project\n";

fn resolve_editor() -> String {
    if let Ok(e) = std::env::var("VISUAL") {
        if !e.trim().is_empty() {
            return e;
        }
    }
    if let Ok(e) = std::env::var("EDITOR") {
        if !e.trim().is_empty() {
            return e;
        }
    }
    if cfg!(windows) {
        "notepad.exe".to_string()
    } else {
        "vi".to_string()
    }
}

/// Drop lines starting with `#?` (the prompt-only comment marker) and trim
/// surrounding whitespace.
fn strip_editor_instructions(raw: &str) -> String {
    raw.lines()
        .filter(|line| !line.trim_start().starts_with("#?"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn preview_first_line(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    if first.chars().count() > 80 {
        let cut: String = first.chars().take(80).collect();
        format!("{cut}…")
    } else {
        first.to_string()
    }
}

fn export_path(target: Option<String>) -> std::path::PathBuf {
    if let Some(t) = target.filter(|s| !s.is_empty()) {
        return std::path::PathBuf::from(t);
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    std::path::PathBuf::from(format!("aonyx-session-{stamp}.md"))
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

    #[test]
    fn strip_editor_instructions_drops_marker_lines() {
        let raw = "#? this is help\n#? second hint\nhello world\nmore text\n";
        let cleaned = strip_editor_instructions(raw);
        assert_eq!(cleaned, "hello world\nmore text");
    }

    #[test]
    fn strip_editor_instructions_treats_empty_input_as_empty() {
        assert!(strip_editor_instructions("#? only hint\n").is_empty());
        assert!(strip_editor_instructions("").is_empty());
    }

    #[test]
    fn preview_first_line_truncates_long_lines() {
        let text = "a".repeat(200);
        let p = preview_first_line(&text);
        assert!(p.chars().count() <= 81);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn preview_first_line_returns_first_line_only() {
        let text = "hello\nworld";
        assert_eq!(preview_first_line(text), "hello");
    }

    #[test]
    fn resolve_editor_falls_back_to_platform_default() {
        // Snapshot env vars and clear them so we hit the fallback branch.
        let visual = std::env::var("VISUAL").ok();
        let editor = std::env::var("EDITOR").ok();
        std::env::remove_var("VISUAL");
        std::env::remove_var("EDITOR");
        let resolved = resolve_editor();
        if cfg!(windows) {
            assert_eq!(resolved, "notepad.exe");
        } else {
            assert_eq!(resolved, "vi");
        }
        if let Some(v) = visual {
            std::env::set_var("VISUAL", v);
        }
        if let Some(e) = editor {
            std::env::set_var("EDITOR", e);
        }
    }

    #[test]
    fn export_path_defaults_to_timestamped_file() {
        let p = export_path(None);
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("aonyx-session-"));
        assert!(name.ends_with(".md"));
    }

    #[test]
    fn export_path_uses_explicit_target_when_provided() {
        let p = export_path(Some("notes/talk.md".into()));
        assert_eq!(p, std::path::PathBuf::from("notes/talk.md"));
    }

    #[test]
    fn slash_command_parses_editor_aliases() {
        for s in ["/editor", "/e"] {
            assert_eq!(
                SlashCommand::parse(s),
                Some(SlashCommand::Editor),
                "for {s}"
            );
        }
    }

    #[test]
    fn slash_command_export_captures_path_argument() {
        match SlashCommand::parse("/export out/transcript.md") {
            Some(SlashCommand::Export(Some(p))) => assert_eq!(p, "out/transcript.md"),
            other => panic!("unexpected: {other:?}"),
        }
        match SlashCommand::parse("/export") {
            Some(SlashCommand::Export(None)) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
