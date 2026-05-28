//! Full-screen TUI built on `ratatui` + `crossterm`.
//!
//! Launched by `aonyx --tui` (Phase B0). The legacy raw stdin/stdout REPL
//! in [`crate::session`] remains the default while this matures.
//!
//! ## Layout
//!
//! ```text
//! ┌── Aonyx Agent ─────────────────── project:Agent-AI ──┐
//! │  conversation viewport (scrollable, streamed)         │
//! │                                                       │
//! ├───────────────────────────────────────────────────────┤
//! │ you> _                                                │  ← composer
//! ├───────────────────────────────────────────────────────┤
//! │ claude-code · claude-sonnet-4-5 · turn 1 · running    │  ← status bar
//! └───────────────────────────────────────────────────────┘
//! ```
//!
//! - **Header** (1 line): app + project name.
//! - **Viewport** (flex): conversation history + tool events; scrollable.
//! - **Composer** (3 lines): single-line input for now.
//! - **Status bar** (1 line): provider · model · turn · runner state.
//!
//! ## Key bindings (V0)
//!
//! - `Enter` → submit message.
//! - `Backspace` → erase last char.
//! - `PgUp` / `PgDn` → scroll viewport.
//! - `Ctrl+C` / `Ctrl+D` / `Esc` → quit.
//!
//! Multi-line composer, history navigation, smart paste, mouse and slash
//! commands land in subsequent B sub-phases.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use aonyx_agent::{AgentRunner, ApprovalPolicy, TurnEvent};
use aonyx_core::{LlmProvider, MemoryStore, Message, Role, SafetyClass};
use aonyx_memory::Palace;
use aonyx_skills::Skill;
use aonyx_tools::ToolRegistry;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
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

    let app = TuiApp {
        runner: Arc::new(runner),
        palace,
        messages,
        project_slug,
        provider_name,
        model_name: model,
        turns: 0,
        composer: String::new(),
        viewport: vec![Line::from(Span::styled(
            "🦦 Aonyx Agent — type a message and press Enter. Esc / Ctrl+C to quit.",
            Style::default().fg(Color::DarkGray),
        ))],
        scroll: 0,
        runner_event_rx: None,
        runner_handle: None,
        runner_active: false,
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

    composer: String,
    viewport: Vec<Line<'static>>,
    scroll: u16,

    runner_event_rx: Option<mpsc::Receiver<TurnEvent>>,
    runner_handle: Option<JoinHandle<aonyx_core::Result<aonyx_agent::TurnResult>>>,
    runner_active: bool,

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

            // ~50 Hz refresh; keystrokes interrupt the wait immediately.
            if event::poll(Duration::from_millis(20))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key.code, key.modifiers).await;
                    }
                    Event::Mouse(_) | Event::Resize(_, _) => { /* ignored in V0 */ }
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
                // Force a fresh line so the next iteration's tool/text starts clean.
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
                self.push_line(Line::from(vec![
                    Span::styled("  ↳ ", Style::default().fg(arrow_color)),
                    Span::styled(
                        format!("{name}: {summary}"),
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
        // Make sure the line we're writing into starts with the bold "aonyx>" header.
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

        let pieces: Vec<&str> = text.split_inclusive('\n').collect();
        for piece in pieces {
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
        // Keep viewport bounded so memory does not grow forever during long
        // sessions; 2000 lines is comfortable on every terminal we care about.
        if self.viewport.len() > 2000 {
            let drop = self.viewport.len() - 2000;
            self.viewport.drain(..drop);
        }
    }

    async fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::Char('c') | KeyCode::Char('d') if ctrl => {
                self.quit = true;
            }
            KeyCode::Esc => {
                self.quit = true;
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_add(8),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(8),
            KeyCode::Enter => {
                let input = std::mem::take(&mut self.composer);
                let trimmed = input.trim();
                if trimmed.is_empty() || self.runner_active {
                    return;
                }
                self.push_line(Line::from(vec![
                    Span::styled("you> ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(trimmed.to_string()),
                ]));
                self.messages
                    .push(Message::new(Role::User, trimmed.to_string()));
                self.start_runner();
            }
            KeyCode::Backspace => {
                self.composer.pop();
            }
            KeyCode::Char(c) => {
                self.composer.push(c);
            }
            _ => {}
        }
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

    fn render(&self, f: &mut Frame<'_>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(3),
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
        ]))
        .block(Block::default().borders(Borders::BOTTOM));
        f.render_widget(header, chunks[0]);

        let viewport = Paragraph::new(Text::from(self.viewport.clone()))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        f.render_widget(viewport, chunks[1]);

        let composer_text = if self.runner_active {
            Line::from(Span::styled(
                "  (waiting for the runner to finish — hit Esc to quit)",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(vec![
                Span::styled(
                    "you> ",
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::White),
                ),
                Span::raw(self.composer.clone()),
                Span::styled("█", Style::default().fg(Color::DarkGray)),
            ])
        };
        let composer = Paragraph::new(composer_text)
            .block(Block::default().borders(Borders::TOP | Borders::BOTTOM));
        f.render_widget(composer, chunks[2]);

        let state = if self.runner_active {
            "running"
        } else {
            "idle"
        };
        let status = Paragraph::new(Line::from(vec![Span::styled(
            format!(
                " {} · {} · turn {} · {} ",
                self.provider_name, self.model_name, self.turns, state
            ),
            Style::default().fg(Color::DarkGray),
        )]));
        f.render_widget(status, chunks[3]);
    }
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
}
