//! Full-screen TUI built on `ratatui` + `crossterm` + `tui-textarea`.
//!
//! Launched by `aonyx --tui`. The legacy raw stdin/stdout REPL in
//! [`crate::session`] is still the default while this matures.
//!
//! ## Layout
//!
//! ```text
//! ┌── 🦦 Aonyx Agent ────────────── project:Agent-AI ────┐
//! │  conversation viewport (scrollable)                   │
//! │  + streaming Markdown + colored tool events           │
//! ├───────────────────────────────────────────────────────┤
//! │ ┌──────────────────────────────────────────────────┐ │
//! │ │ you> _                                           │ │  ← composer
//! │ │   (Shift+Enter for newline, ↑/↓ history)         │ │   (multi-line)
//! │ └──────────────────────────────────────────────────┘ │
//! ├───────────────────────────────────────────────────────┤
//! │ claude-code · claude-sonnet-4-5 · turn 1 · running    │  ← status bar
//! └───────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key bindings (B1)
//!
//! - `Enter` → submit message (or run a slash command)
//! - `Shift+Enter` / `Alt+Enter` → insert newline in the composer
//! - `↑` / `↓` → step through the user-message history (only when the cursor
//!   is on the top / bottom line of the composer)
//! - `PgUp` / `PgDn` → scroll viewport
//! - `Backspace`, `Delete`, arrows, `Home`, `End`, `Ctrl+W` (word back),
//!   `Ctrl+U` (clear) → standard text edit, handled by `tui-textarea`.
//! - `Esc` / `Ctrl+C` / `Ctrl+D` → quit.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use aonyx_agent::{AgentRunner, ApprovalPolicy, TurnEvent};
use aonyx_core::{LlmProvider, MemoryStore, Message, Role, SafetyClass};
use aonyx_memory::Palace;
use aonyx_skills::Skill;
use aonyx_tools::ToolRegistry;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tui_textarea::{CursorMove, TextArea};

use crate::session::SlashCommand;

const HISTORY_MAX: usize = 200;
const VIEWPORT_MAX_LINES: usize = 2000;
const MIN_COMPOSER_HEIGHT: u16 = 3;
const MAX_COMPOSER_HEIGHT: u16 = 10;

/// Construct + run a TUI session. Returns when the user quits or the
/// terminal disconnects.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    provider: Arc<dyn LlmProvider>,
    palace: Palace,
    model: String,
    max_iterations: usize,
    system_prompt: Option<String>,
    project_slug: String,
    skills: Vec<Skill>,
    provider_name: String,
) -> anyhow::Result<()> {
    let runner = AgentRunner::new(provider, ToolRegistry::default_set(), model.clone())
        .with_max_iterations(max_iterations)
        .with_approval(ApprovalPolicy::DenyDestructive)
        .with_skills(skills)
        .with_project(&project_slug);

    let mut messages: Vec<Message> = Vec::new();
    if let Some(p) = system_prompt {
        messages.push(Message::new(Role::System, p));
    }

    let mut composer = TextArea::default();
    composer.set_block(
        Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    composer.set_cursor_line_style(Style::default());
    composer.set_placeholder_text("type a message — Enter to send, Shift+Enter for newline");

    let app = TuiApp {
        runner: Arc::new(runner),
        palace,
        messages,
        project_slug,
        provider_name,
        model_name: model,
        turns: 0,
        composer,
        viewport: vec![Line::from(Span::styled(
            "🦦 Aonyx Agent — Shift+Enter = newline · ↑/↓ history · Esc to quit · /help for commands",
            Style::default().fg(Color::DarkGray),
        ))],
        scroll: 0,
        history: Vec::new(),
        history_cursor: None,
        scratch: Vec::new(),
        runner_event_rx: None,
        runner_handle: None,
        runner_active: false,
        show_tool_details: false,
        quit: false,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = app.event_loop(&mut terminal).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

struct TuiApp {
    runner: Arc<AgentRunner>,
    palace: Palace,
    messages: Vec<Message>,
    project_slug: String,
    provider_name: String,
    model_name: String,
    turns: u32,

    composer: TextArea<'static>,
    viewport: Vec<Line<'static>>,
    scroll: u16,

    history: Vec<String>,
    history_cursor: Option<usize>,
    scratch: Vec<String>,

    runner_event_rx: Option<mpsc::Receiver<TurnEvent>>,
    runner_handle: Option<JoinHandle<aonyx_core::Result<aonyx_agent::TurnResult>>>,
    runner_active: bool,
    show_tool_details: bool,

    quit: bool,
}

impl TuiApp {
    async fn event_loop(
        mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> anyhow::Result<()> {
        while !self.quit {
            terminal.draw(|f| self.render(f))?;
            self.poll_runner().await;

            if event::poll(Duration::from_millis(20))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key).await;
                    }
                    Event::Mouse(_) | Event::Resize(_, _) => { /* ignored in B1 */ }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    async fn poll_runner(&mut self) {
        if self.runner_event_rx.is_none() {
            return;
        }

        loop {
            let next = self
                .runner_event_rx
                .as_mut()
                .and_then(|rx| rx.try_recv().ok());
            match next {
                Some(ev) => self.apply_event(ev),
                None => break,
            }
        }

        if let Some(handle) = &self.runner_handle {
            if handle.is_finished() {
                let handle = self.runner_handle.take().expect("checked above");
                match handle.await {
                    Ok(Ok(turn)) => {
                        self.messages = turn.messages;
                        self.turns += 1;
                        let summary = self
                            .messages
                            .iter()
                            .rev()
                            .find(|m| m.role == Role::User)
                            .map(|m| m.content.clone())
                            .unwrap_or_default();
                        let _ = self.palace.diary_append(&self.project_slug, &summary).await;
                    }
                    Ok(Err(e)) => self.push_line(error_line(format!("{e}"))),
                    Err(e) => self.push_line(error_line(format!("join: {e}"))),
                }
                self.runner_event_rx = None;
                self.runner_active = false;
            }
        }
    }

    fn apply_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::AssistantDelta(text) => {
                self.append_to_assistant_line(&text);
            }
            TurnEvent::AssistantMessageEnd => {
                if !self.viewport.is_empty() {
                    let last_empty = self
                        .viewport
                        .last()
                        .map(|l| l.spans.is_empty())
                        .unwrap_or(false);
                    if !last_empty {
                        self.viewport.push(Line::default());
                    }
                }
            }
            TurnEvent::ToolStart { name, args, class } => {
                let dot_color = match class {
                    SafetyClass::Safe => Color::Cyan,
                    SafetyClass::Caution => Color::Yellow,
                    SafetyClass::Destructive => Color::Red,
                };
                let preview = abbreviate_value(&args, 80);
                self.push_line(Line::from(vec![
                    Span::styled("● ", Style::default().fg(dot_color)),
                    Span::styled(name, Style::default().fg(Color::Cyan)),
                    Span::styled(format!("({preview})"), Style::default().fg(Color::DarkGray)),
                ]));
            }
            TurnEvent::ToolEnd { name, ok, summary } => {
                let arrow_color = if ok { Color::Green } else { Color::Red };
                let trimmed = if self.show_tool_details {
                    summary
                } else {
                    truncate(&summary, 120)
                };
                self.push_line(Line::from(vec![
                    Span::styled("  ↳ ", Style::default().fg(arrow_color)),
                    Span::styled(
                        format!("{name}: {trimmed}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            TurnEvent::ToolRejected { name, class } => {
                self.push_line(Line::from(vec![
                    Span::styled("  ✗ rejected: ", Style::default().fg(Color::Red)),
                    Span::styled(
                        format!("{name} ({class:?})"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            TurnEvent::IterationStart(n) if n > 1 => {
                self.push_line(Line::from(Span::styled(
                    format!("[iter {n}]"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            TurnEvent::Done {
                max_iterations_hit: true,
                iterations,
            } => {
                self.push_line(Line::from(Span::styled(
                    format!("(loop hit max_iterations = {iterations})"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            _ => {}
        }
    }

    fn append_to_assistant_line(&mut self, text: &str) {
        let needs_header = match self.viewport.last() {
            None => true,
            Some(l) => !l.spans.iter().any(|s| s.content.contains("aonyx>")),
        };
        if needs_header {
            self.viewport.push(Line::from(vec![Span::styled(
                "aonyx> ",
                Style::default().add_modifier(Modifier::BOLD),
            )]));
        }

        for piece in text.split_inclusive('\n') {
            let (chunk, has_newline) = match piece.strip_suffix('\n') {
                Some(c) => (c, true),
                None => (piece, false),
            };
            if !chunk.is_empty() {
                if let Some(last) = self.viewport.last_mut() {
                    last.spans.push(Span::raw(chunk.to_string()));
                }
            }
            if has_newline {
                self.viewport.push(Line::default());
            }
        }
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.viewport.push(line);
        if self.viewport.len() > VIEWPORT_MAX_LINES {
            let drop = self.viewport.len() - VIEWPORT_MAX_LINES;
            self.viewport.drain(..drop);
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        use KeyCode::*;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            Esc => {
                self.quit = true;
            }
            Char('c') | Char('d') if ctrl => {
                self.quit = true;
            }
            PageUp => self.scroll = self.scroll.saturating_add(8),
            PageDown => self.scroll = self.scroll.saturating_sub(8),

            Up if self.composer_at_top() && !shift => self.history_prev(),
            Down if self.composer_at_bottom() && !shift => self.history_next(),

            Enter if shift || alt => {
                self.composer.insert_newline();
            }
            Enter => {
                self.submit_composer().await;
            }

            _ => {
                // Hand the event over to tui-textarea for typing, cursor
                // motion, backspace, etc. Discard the boolean it returns
                // ("did this modify the buffer") — we re-render every frame.
                let _ = self.composer.input(key);
            }
        }
    }

    fn composer_at_top(&self) -> bool {
        self.composer.cursor().0 == 0
    }

    fn composer_at_bottom(&self) -> bool {
        let (row, _) = self.composer.cursor();
        row >= self.composer.lines().len().saturating_sub(1)
    }

    fn set_composer_content(&mut self, content: &str) {
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(String::from).collect()
        };
        let mut next = TextArea::new(lines);
        next.set_block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        next.set_cursor_line_style(Style::default());
        next.set_placeholder_text("type a message — Enter to send, Shift+Enter for newline");
        next.move_cursor(CursorMove::Bottom);
        next.move_cursor(CursorMove::End);
        self.composer = next;
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let new_idx = match self.history_cursor {
            None => self.history.len() - 1,
            Some(0) => return,
            Some(n) => n - 1,
        };
        if self.history_cursor.is_none() {
            self.scratch = self.composer.lines().to_vec();
        }
        self.history_cursor = Some(new_idx);
        let value = self.history[new_idx].clone();
        self.set_composer_content(&value);
    }

    fn history_next(&mut self) {
        match self.history_cursor {
            None => {}
            Some(n) if n + 1 >= self.history.len() => {
                let scratch = self.scratch.clone().join("\n");
                self.history_cursor = None;
                self.set_composer_content(&scratch);
            }
            Some(n) => {
                self.history_cursor = Some(n + 1);
                let value = self.history[n + 1].clone();
                self.set_composer_content(&value);
            }
        }
    }

    async fn submit_composer(&mut self) {
        if self.runner_active {
            return;
        }
        let content = self.composer.lines().join("\n");
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return;
        }

        // Track in history (skip exact duplicates of the previous entry).
        if self.history.last().map(String::as_str) != Some(trimmed) {
            self.history.push(trimmed.to_string());
            if self.history.len() > HISTORY_MAX {
                let drop = self.history.len() - HISTORY_MAX;
                self.history.drain(..drop);
            }
        }
        self.history_cursor = None;
        self.scratch.clear();

        if let Some(cmd) = SlashCommand::parse(trimmed) {
            self.handle_slash(cmd).await;
        } else {
            self.push_line(Line::from(vec![
                Span::styled("you> ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(trimmed.to_string()),
            ]));
            self.messages
                .push(Message::new(Role::User, trimmed.to_string()));
            self.start_runner();
        }

        self.set_composer_content("");
    }

    fn start_runner(&mut self) {
        let (tx, rx) = mpsc::channel::<TurnEvent>(256);
        let runner = Arc::clone(&self.runner);
        let messages = self.messages.clone();
        let handle = tokio::spawn(async move { runner.run_streaming(messages, tx).await });
        self.runner_event_rx = Some(rx);
        self.runner_handle = Some(handle);
        self.runner_active = true;
    }

    async fn handle_slash(&mut self, cmd: SlashCommand) {
        match cmd {
            SlashCommand::Quit => self.quit = true,
            SlashCommand::Clear | SlashCommand::New => {
                let system = self
                    .messages
                    .first()
                    .filter(|m| m.role == Role::System)
                    .cloned();
                self.messages.clear();
                if let Some(s) = system {
                    self.messages.push(s);
                }
                self.turns = 0;
                self.viewport.clear();
                self.push_dim("(history cleared)");
            }
            SlashCommand::Help => {
                for line in HELP_LINES {
                    self.push_dim(line);
                }
            }
            SlashCommand::Models => {
                self.push_dim(&format!(
                    "active: {} · {}",
                    self.provider_name, self.model_name
                ));
                self.push_dim(
                    "available: anthropic · openai · openrouter · ollama · lm-studio · claude-code",
                );
                self.push_dim("switch with: edit ~/.aonyx/config.toml (live switch in V0.3)");
            }
            SlashCommand::Sessions => {
                self.push_dim("single-session mode (multi-session lands in Phase D)");
            }
            SlashCommand::Export(target) => {
                let path = export_path(target);
                match self.export_markdown(&path).await {
                    Ok(()) => self.push_dim(&format!(
                        "exported: {} ({} messages)",
                        path.display(),
                        self.messages.len()
                    )),
                    Err(e) => self.push_line(error_line(format!("export failed: {e}"))),
                }
            }
            SlashCommand::Details => {
                self.show_tool_details = !self.show_tool_details;
                let state = if self.show_tool_details { "on" } else { "off" };
                self.push_dim(&format!("tool details: {state}"));
            }
            SlashCommand::Thinking => {
                self.push_dim("reasoning visibility lands in Phase E");
            }
            SlashCommand::Editor => {
                self.push_dim("`/editor` runs in legacy mode (`aonyx` without --tui) for now");
            }
            SlashCommand::Init => {
                let path = std::path::PathBuf::from("agent.yaml");
                if path.exists() {
                    self.push_dim(&format!(
                        "{} already exists — leaving it alone",
                        path.display()
                    ));
                } else {
                    let yaml = format!(
                        "# Aonyx Agent — per-project configuration\n\
                         persona: \"You are an Aonyx agent helping with {} .\"\n\
                         system_prompt: |\n  Be concise. Cite sources. Confirm destructive actions.\n\
                         preferred_provider: {}\n\
                         preferred_model: {}\n",
                        self.project_slug, self.provider_name, self.model_name
                    );
                    match tokio::fs::write(&path, yaml).await {
                        Ok(()) => self.push_dim(&format!("created: {}", path.display())),
                        Err(e) => self.push_line(error_line(format!("init failed: {e}"))),
                    }
                }
            }
        }
    }

    async fn export_markdown(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut out = String::new();
        out.push_str(&format!(
            "# Aonyx Agent session — {project}\n\n",
            project = self.project_slug
        ));
        out.push_str(&format!(
            "_provider: {} · model: {} · turns: {}_\n\n---\n\n",
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

    fn push_dim(&mut self, text: &str) {
        self.push_line(Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(Color::DarkGray),
        )));
    }

    fn composer_height(&self) -> u16 {
        let lines = self.composer.lines().len() as u16;
        // +2 for the top + bottom border
        lines
            .saturating_add(2)
            .clamp(MIN_COMPOSER_HEIGHT, MAX_COMPOSER_HEIGHT)
    }

    fn render(&mut self, f: &mut Frame<'_>) {
        let composer_h = self.composer_height();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(composer_h),
                Constraint::Length(1),
            ])
            .split(f.area());

        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                "🦦 Aonyx Agent",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  project:{}", self.project_slug),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        f.render_widget(header, chunks[0]);

        let viewport = Paragraph::new(Text::from(self.viewport.clone()))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        f.render_widget(viewport, chunks[1]);

        if self.runner_active {
            let blocker = Paragraph::new(Line::from(Span::styled(
                "  (runner busy — message will queue after it finishes)",
                Style::default().fg(Color::DarkGray),
            )))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            f.render_widget(blocker, chunks[2]);
        } else {
            f.render_widget(&self.composer, chunks[2]);
        }

        let state = if self.runner_active {
            "running"
        } else {
            "idle"
        };
        let details = if self.show_tool_details {
            " · details:on"
        } else {
            ""
        };
        let status = Paragraph::new(Line::from(Span::styled(
            format!(
                " {} · {} · turn {} · {}{} ",
                self.provider_name, self.model_name, self.turns, state, details
            ),
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(status, chunks[3]);
    }
}

const HELP_LINES: &[&str] = &[
    "available commands:",
    "  /quit /q /exit       exit",
    "  /clear /reset /new   reset conversation (keeps system prompt)",
    "  /help /?             this list",
    "  /models /m           active provider + model",
    "  /sessions /s         multi-session UI (Phase D)",
    "  /export [path]       dump the conversation to Markdown",
    "  /details             toggle verbose tool output",
    "  /thinking            reasoning visibility (Phase E)",
    "  /editor /e           legacy-mode only for now",
    "  /init                drop an agent.yaml in the project root",
    "keys: Shift+Enter newline · ↑/↓ history · PgUp/PgDn scroll · Esc quit",
];

fn export_path(target: Option<String>) -> std::path::PathBuf {
    if let Some(t) = target.filter(|s| !s.is_empty()) {
        return std::path::PathBuf::from(t);
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    std::path::PathBuf::from(format!("aonyx-session-{stamp}.md"))
}

fn abbreviate_value(value: &serde_json::Value, max_chars: usize) -> String {
    let mut s = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    s = s.replace('\n', " ");
    truncate(&s, max_chars)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    } else {
        s.to_string()
    }
}

fn error_line(text: String) -> Line<'static> {
    Line::from(vec![
        Span::styled("[error] ", Style::default().fg(Color::Red)),
        Span::raw(text),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviate_value_truncates_long_strings() {
        let v = serde_json::Value::String("x".repeat(200));
        let s = abbreviate_value(&v, 50);
        assert!(s.chars().count() <= 51);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn error_line_starts_with_marker() {
        let line = error_line("boom".into());
        assert!(line.spans[0].content.contains("[error]"));
        assert!(line.spans[1].content.contains("boom"));
    }

    #[test]
    fn truncate_keeps_short_strings() {
        assert_eq!(truncate("hello", 80), "hello");
        assert!(truncate(&"x".repeat(200), 50).ends_with('…'));
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
        let p = export_path(Some("transcript.md".into()));
        assert_eq!(p, std::path::PathBuf::from("transcript.md"));
    }
}
