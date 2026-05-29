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
use std::time::{Duration, Instant};

use aonyx_agent::{AgentRunner, ApprovalPolicy, TurnEvent};
use aonyx_core::{LlmProvider, MemoryStore, Message, Role, SafetyClass};
use aonyx_memory::{Palace, SessionId, SessionStore, SqliteSessionStore};
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
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tui_textarea::{CursorMove, TextArea};

use crate::session::SlashCommand;
use crate::theme::{self, Theme};

const HISTORY_MAX: usize = 200;
const VIEWPORT_MAX_LINES: usize = 2000;
const MIN_COMPOSER_HEIGHT: u16 = 3;
const MAX_COMPOSER_HEIGHT: u16 = 10;
const SUGGESTION_LIMIT: usize = 8;
const FILE_CACHE_LIMIT: usize = 5000;
const FILE_CACHE_MAX_DEPTH: usize = 8;

const SLASH_CANDIDATES: &[&str] = &[
    "/quit",
    "/clear",
    "/new",
    "/help",
    "/models",
    "/sessions",
    "/export",
    "/details",
    "/thinking",
    "/editor",
    "/init",
];

/// Which prefix character opened the suggestions popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// `@path` — fuzzy file picker over the cwd.
    At,
    /// `/cmd` — slash command picker.
    Slash,
}

/// Frames for the running-runner spinner. Braille dots feel lively without
/// burning CPU on the redraw.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Pulse frames for in-flight tool dots — small to keep eye-strain low.
const PULSE_FRAMES: &[&str] = &["●", "◉", "○", "◉"];

/// Construct + run a TUI session. Returns when the user quits or the
/// terminal disconnects.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    provider: Arc<dyn LlmProvider>,
    palace: Palace,
    model: String,
    max_iterations: usize,
    _system_prompt: Option<String>,
    project_slug: String,
    skills: Vec<Skill>,
    provider_name: String,
    session_store: SqliteSessionStore,
    session_id: SessionId,
    session_messages: Vec<Message>,
    session_turns: u32,
    theme_name: Option<String>,
    show_thinking: bool,
    desktop_notifications: bool,
) -> anyhow::Result<()> {
    let runner = AgentRunner::new(provider, ToolRegistry::default_set(), model.clone())
        .with_max_iterations(max_iterations)
        .with_approval(ApprovalPolicy::DenyDestructive)
        .with_skills(skills)
        .with_project(&project_slug);

    let messages: Vec<Message> = session_messages;

    let mut composer = TextArea::default();
    composer.set_block(
        Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    composer.set_cursor_line_style(Style::default());
    composer.set_placeholder_text("type a message — Enter to send, Shift+Enter for newline");

    let active_theme = theme_name
        .as_deref()
        .map(theme::by_name)
        .unwrap_or(theme::DEFAULT);

    let app = TuiApp {
        runner: Arc::new(runner),
        palace,
        messages,
        project_slug,
        provider_name,
        model_name: model,
        turns: session_turns,
        session_store,
        session_id,
        theme: active_theme,
        show_thinking,
        desktop_notifications,
        composer,
        viewport: vec![Line::from(Span::styled(
            "🦦 Aonyx Agent — Shift+Enter = newline · ↑/↓ history · Esc to quit · /help for commands",
            Style::default().fg(Color::DarkGray),
        ))],
        scroll: 0,
        auto_scroll: true,
        viewport_height: 0,
        history: Vec::new(),
        history_cursor: None,
        scratch: Vec::new(),
        runner_event_rx: None,
        runner_handle: None,
        runner_active: false,
        show_tool_details: false,
        tick: 0,
        thinking_line: None,
        first_delta_received: false,
        current_assistant_text: String::new(),
        assistant_msg_start: None,
        suggestions: Vec::new(),
        suggestion_idx: 0,
        suggestion_kind: None,
        suggestion_trigger_pos: 0,
        file_cache: None,
        turn_started_at: None,
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
    session_store: SqliteSessionStore,
    session_id: SessionId,
    theme: Theme,
    show_thinking: bool,
    desktop_notifications: bool,

    composer: TextArea<'static>,
    viewport: Vec<Line<'static>>,
    scroll: u16,
    /// `true` until the user explicitly scrolls away (PgUp). Re-enabled on
    /// PgDn when the user reaches the bottom or on End.
    auto_scroll: bool,
    /// Updated on every `render()`; the auto-scroll math needs it.
    viewport_height: u16,

    history: Vec<String>,
    history_cursor: Option<usize>,
    scratch: Vec<String>,

    runner_event_rx: Option<mpsc::Receiver<TurnEvent>>,
    runner_handle: Option<JoinHandle<aonyx_core::Result<aonyx_agent::TurnResult>>>,
    runner_active: bool,
    show_tool_details: bool,
    /// Monotonic tick incremented each event-loop iteration; drives spinner
    /// + pulse animations without burning CPU.
    tick: u64,
    /// Index in `viewport` of the "💭 thinking…" placeholder, if any.
    thinking_line: Option<usize>,
    /// `true` once the runner has sent its first AssistantDelta — used to
    /// retire the thinking placeholder.
    first_delta_received: bool,
    /// Raw text streamed for the assistant message currently in flight.
    /// Rendered as Markdown by `tui-markdown` at `AssistantMessageEnd`.
    current_assistant_text: String,
    /// `viewport` index where the current assistant message started. The
    /// raw streamed lines from that index up to the end are replaced by the
    /// Markdown-rendered lines at `AssistantMessageEnd`.
    assistant_msg_start: Option<usize>,

    /// Currently-displayed suggestions; empty when the popup is closed.
    suggestions: Vec<String>,
    /// Index of the currently highlighted suggestion.
    suggestion_idx: usize,
    /// What triggered the popup (`@` or `/`).
    suggestion_kind: Option<Trigger>,
    /// Byte position of the trigger character in the composer text.
    suggestion_trigger_pos: usize,
    /// Lazily-populated walk of cwd file paths for `@` suggestions.
    file_cache: Option<Vec<String>>,
    /// Wall-clock start of the current runner task, used to gate desktop
    /// notifications on long turns.
    turn_started_at: Option<Instant>,

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

            // Shorter poll while the runner is busy so the spinner stays
            // smooth (≈ 80 ms per frame ≈ 12 fps), longer while idle so we
            // don't tax the CPU.
            let timeout = if self.runner_active {
                Duration::from_millis(80)
            } else {
                Duration::from_millis(50)
            };

            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key).await;
                    }
                    Event::Mouse(_) | Event::Resize(_, _) => { /* ignored in B1 */ }
                    _ => {}
                }
            }
            self.tick = self.tick.wrapping_add(1);
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
                        // Persist the session so we can resume after crashes / restart.
                        let _ = self
                            .session_store
                            .update(self.session_id, self.messages.clone(), self.turns)
                            .await;
                        self.maybe_notify("Aonyx Agent", "Turn finished", Duration::from_secs(5));
                    }
                    Ok(Err(e)) => {
                        self.maybe_notify("Aonyx Agent (error)", &format!("{e}"), Duration::ZERO);
                        self.push_line(error_line(format!("{e}")));
                    }
                    Err(e) => {
                        self.maybe_notify(
                            "Aonyx Agent (error)",
                            &format!("join: {e}"),
                            Duration::ZERO,
                        );
                        self.push_line(error_line(format!("join: {e}")));
                    }
                }
                self.runner_event_rx = None;
                self.runner_active = false;
                self.turn_started_at = None;
                self.retire_thinking_line();
            }
        }
    }

    fn retire_thinking_line(&mut self) {
        if let Some(idx) = self.thinking_line.take() {
            if idx < self.viewport.len() {
                self.viewport.remove(idx);
            }
        }
        self.first_delta_received = false;
    }

    /// Replace the raw streamed assistant lines with Markdown-rendered ones.
    fn finalize_assistant_message(&mut self) {
        let Some(start) = self.assistant_msg_start else {
            return;
        };
        if self.current_assistant_text.trim().is_empty() {
            return;
        }
        if start > self.viewport.len() {
            return;
        }

        // Drop the raw lines we streamed in between [start, end).
        self.viewport.truncate(start);

        // Re-emit a coloured "aonyx>" header line so it stands out from the
        // surrounding Markdown content.
        self.viewport.push(Line::from(Span::styled(
            "aonyx>",
            Style::default()
                .fg(self.theme.assistant_prefix)
                .add_modifier(Modifier::BOLD),
        )));

        // Render the buffered text as Markdown and push every produced line.
        let rendered = tui_markdown::from_str(&self.current_assistant_text);
        for line in rendered.lines.into_iter() {
            self.viewport.push(line_to_static(line));
        }
    }

    fn apply_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::AssistantDelta(text) => {
                if !self.first_delta_received {
                    self.retire_thinking_line();
                    self.first_delta_received = true;
                    // Remember where this assistant message starts so we can
                    // replace the raw streamed lines with Markdown-rendered
                    // ones at AssistantMessageEnd.
                    self.assistant_msg_start = Some(self.viewport.len());
                }
                self.current_assistant_text.push_str(&text);
                self.append_to_assistant_line(&text);
            }
            TurnEvent::AssistantMessageEnd => {
                self.finalize_assistant_message();
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
                self.first_delta_received = false;
                self.assistant_msg_start = None;
                self.current_assistant_text.clear();
            }
            TurnEvent::ToolStart { name, args, class } => {
                self.retire_thinking_line();
                self.first_delta_received = true;
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
                Style::default()
                    .fg(self.theme.assistant_prefix)
                    .add_modifier(Modifier::BOLD),
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
            if let Some(idx) = self.thinking_line {
                self.thinking_line = idx.checked_sub(drop);
            }
        }
    }

    fn push_thinking_line(&mut self) {
        let span = Span::styled(
            "  💭 thinking…",
            Style::default()
                .fg(self.theme.thinking)
                .add_modifier(Modifier::ITALIC),
        );
        self.viewport.push(Line::from(span));
        self.thinking_line = Some(self.viewport.len() - 1);
        self.first_delta_received = false;
    }

    fn clamp_scroll_and_maybe_resume_auto(&mut self) {
        let max = self.max_scroll();
        if self.scroll >= max {
            self.scroll = max;
            self.auto_scroll = true;
        }
    }

    fn max_scroll(&self) -> u16 {
        let total = self.viewport.len() as u32;
        let visible = self.viewport_height as u32;
        (total.saturating_sub(visible)) as u16
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        use KeyCode::*;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let suggestions_open = !self.suggestions.is_empty();

        match key.code {
            // While the suggestions popup is open, Esc just closes it.
            Esc if suggestions_open => {
                self.dismiss_suggestions();
            }
            Esc => {
                self.quit = true;
            }
            Char('c') | Char('d') if ctrl => {
                self.quit = true;
            }
            PageUp => {
                self.auto_scroll = false;
                self.scroll = self.scroll.saturating_sub(8);
            }
            PageDown => {
                self.scroll = self.scroll.saturating_add(8);
                self.clamp_scroll_and_maybe_resume_auto();
            }
            End => {
                self.auto_scroll = true;
            }
            Home => {
                self.auto_scroll = false;
                self.scroll = 0;
            }

            // While suggestions are open, ↑/↓ navigate the popup.
            Up if suggestions_open => {
                if self.suggestion_idx > 0 {
                    self.suggestion_idx -= 1;
                }
            }
            Down if suggestions_open => {
                if self.suggestion_idx + 1 < self.suggestions.len() {
                    self.suggestion_idx += 1;
                }
            }
            // Tab accepts the highlighted suggestion.
            Tab if suggestions_open => {
                self.accept_suggestion();
            }

            Up if self.composer_at_top() && !shift => self.history_prev(),
            Down if self.composer_at_bottom() && !shift => self.history_next(),

            Enter if shift || alt => {
                self.composer.insert_newline();
                self.update_suggestions();
            }
            Enter => {
                self.submit_composer().await;
                self.dismiss_suggestions();
            }

            _ => {
                let _ = self.composer.input(key);
                self.update_suggestions();
            }
        }
    }

    fn update_suggestions(&mut self) {
        let text = self.composer.lines().join("\n");
        let cursor_byte = cursor_byte_offset(&self.composer);
        match detect_trigger(&text, cursor_byte) {
            Some((trigger, trigger_pos, query)) => {
                self.suggestion_kind = Some(trigger);
                self.suggestion_trigger_pos = trigger_pos;

                let pool: Vec<String> = match trigger {
                    Trigger::At => self.file_candidates(),
                    Trigger::Slash => SLASH_CANDIDATES.iter().map(|s| (*s).to_string()).collect(),
                };

                let suggestions = if query.is_empty() {
                    pool.into_iter().take(SUGGESTION_LIMIT).collect()
                } else {
                    fuzzy_top(&query, &pool, SUGGESTION_LIMIT)
                };

                self.suggestions = suggestions;
                if self.suggestion_idx >= self.suggestions.len() {
                    self.suggestion_idx = 0;
                }
            }
            None => self.dismiss_suggestions(),
        }
    }

    fn dismiss_suggestions(&mut self) {
        self.suggestions.clear();
        self.suggestion_idx = 0;
        self.suggestion_kind = None;
    }

    fn accept_suggestion(&mut self) {
        let Some(selected) = self.suggestions.get(self.suggestion_idx).cloned() else {
            return;
        };
        let Some(trigger) = self.suggestion_kind else {
            return;
        };
        let trigger_pos = self.suggestion_trigger_pos;
        let text = self.composer.lines().join("\n");
        let cursor_byte = cursor_byte_offset(&self.composer);

        // Build the replacement: keep everything up to and including the
        // trigger char (for `@`) or just up to the trigger (for `/`, since
        // the suggestion already starts with `/`).
        let mut new_text = String::new();
        match trigger {
            Trigger::At => {
                new_text.push_str(&text[..=trigger_pos.min(text.len() - 1)]);
                new_text.push_str(&selected);
            }
            Trigger::Slash => {
                new_text.push_str(&text[..trigger_pos]);
                new_text.push_str(&selected);
            }
        }
        new_text.push(' ');
        if cursor_byte <= text.len() {
            new_text.push_str(&text[cursor_byte..]);
        }
        self.set_composer_content(&new_text);
        self.dismiss_suggestions();
    }

    fn file_candidates(&mut self) -> Vec<String> {
        if self.file_cache.is_none() {
            let base = std::env::current_dir().unwrap_or_else(|_| ".".into());
            self.file_cache = Some(collect_files(&base, FILE_CACHE_MAX_DEPTH, FILE_CACHE_LIMIT));
        }
        self.file_cache.clone().unwrap_or_default()
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

        // Inline bash: `!cmd` runs locally and prints the output back into the
        // viewport + injects it as a system message so the next turn can use it.
        if let Some(cmd) = trimmed.strip_prefix('!') {
            self.handle_bash_inline(cmd.trim()).await;
            self.set_composer_content("");
            return;
        }

        if let Some(cmd) = SlashCommand::parse(trimmed) {
            self.handle_slash(cmd).await;
        } else {
            // `@filename` references: pull file content into the conversation as
            // a system message + show a `📎 loaded:` line in the viewport.
            let (display_text, refs) = extract_refs(trimmed);
            self.push_line(Line::from(vec![
                Span::styled(
                    "you> ",
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(self.theme.user_prefix),
                ),
                Span::raw(display_text.clone()),
            ]));
            if !refs.is_empty() {
                let resolved = resolve_refs(&refs).await;
                for (path, result) in &resolved {
                    match result {
                        Ok(text) => {
                            self.push_dim(&format!("  📎 loaded: {path} ({} bytes)", text.len()));
                        }
                        Err(e) => {
                            self.push_line(error_line(format!("📎 {path}: {e}")));
                        }
                    }
                }
                if let Some(ctx_msg) = build_refs_message(&resolved) {
                    self.messages.push(ctx_msg);
                }
            }
            self.messages.push(Message::new(Role::User, display_text));
            self.push_thinking_line();
            self.auto_scroll = true;
            self.start_runner();
        }

        self.set_composer_content("");
    }

    async fn handle_bash_inline(&mut self, cmd: &str) {
        if cmd.is_empty() {
            self.push_dim("(empty bash command — try `!ls` or `!git status`)");
            return;
        }
        self.push_line(Line::from(vec![
            Span::styled(
                "you> ",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(self.theme.user_prefix),
            ),
            Span::styled(format!("!{cmd}"), Style::default().fg(Color::Yellow)),
        ]));
        match run_bash(cmd).await {
            Ok(out) => {
                self.push_dim(&format!("  $ {cmd}"));
                for line in out.lines() {
                    self.push_line(Line::from(Span::raw(line.to_string())));
                }
                self.messages.push(Message::new(
                    Role::System,
                    format!("User ran `!{cmd}` in the shell. Output:\n```\n{out}\n```"),
                ));
            }
            Err(e) => {
                self.push_line(error_line(format!("bash: {e}")));
            }
        }
        self.auto_scroll = true;
    }

    fn start_runner(&mut self) {
        let (tx, rx) = mpsc::channel::<TurnEvent>(256);
        let runner = Arc::clone(&self.runner);
        let messages = self.messages.clone();
        let handle = tokio::spawn(async move { runner.run_streaming(messages, tx).await });
        self.runner_event_rx = Some(rx);
        self.runner_handle = Some(handle);
        self.runner_active = true;
        self.turn_started_at = Some(Instant::now());
    }

    fn maybe_notify(&self, summary: &str, body: &str, min_elapsed: Duration) {
        if !self.desktop_notifications {
            return;
        }
        if let Some(started) = self.turn_started_at {
            if started.elapsed() < min_elapsed {
                return;
            }
        }
        let _ = notify_rust::Notification::new()
            .summary(summary)
            .body(body)
            .timeout(notify_rust::Timeout::Milliseconds(4000))
            .show();
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
                // /new starts a brand-new persisted session; /clear is a soft
                // reset of the same row. They both clear the viewport but only
                // /new rotates the session id.
                if matches!(cmd, SlashCommand::New) {
                    if let Ok(created) = self
                        .session_store
                        .create(&self.project_slug, self.messages.clone())
                        .await
                    {
                        self.session_id = created.id;
                        self.push_dim(&format!("(new session #{})", created.id));
                    }
                } else {
                    self.push_dim("(history cleared)");
                }
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
                match self
                    .session_store
                    .list_by_project(&self.project_slug, 20)
                    .await
                {
                    Ok(list) if list.is_empty() => self.push_dim("(no other sessions yet)"),
                    Ok(list) => {
                        self.push_dim(&format!(
                            "{} session(s) for project '{}':",
                            list.len(),
                            self.project_slug
                        ));
                        for (i, s) in list.iter().enumerate() {
                            let marker = if s.id == self.session_id { "▸" } else { " " };
                            let line = format!(
                                "{marker} [{:>2}] {} · {} turn(s) · {}",
                                i + 1,
                                s.updated_at.format("%Y-%m-%d %H:%M"),
                                s.turns,
                                s.title
                            );
                            self.push_dim(&line);
                        }
                        self.push_dim("(switch UI lands in Phase D.5)");
                    }
                    Err(e) => self.push_line(error_line(format!("list sessions: {e}"))),
                }
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
                self.show_thinking = !self.show_thinking;
                let state = if self.show_thinking { "on" } else { "off" };
                self.push_dim(&format!(
                    "reasoning visibility: {state} (requires a provider that emits thinking blocks)"
                ));
            }
            SlashCommand::Themes(target) => match target {
                Some(name) => {
                    let new_theme = theme::by_name(&name);
                    let resolved_to_default = !name.eq_ignore_ascii_case(new_theme.name);
                    self.theme = new_theme;
                    if resolved_to_default {
                        self.push_dim(&format!(
                            "unknown theme '{name}' — staying on {}",
                            new_theme.name
                        ));
                    } else {
                        self.push_dim(&format!("theme: {}", new_theme.name));
                    }
                }
                None => {
                    self.push_dim(&format!(
                        "active theme: {} · available: {}",
                        self.theme.name,
                        theme::available_names().join(" · ")
                    ));
                }
            },
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
        let suggestions_h = if self.suggestions.is_empty() {
            0
        } else {
            (self.suggestions.len() as u16 + 2).clamp(3, 10)
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(suggestions_h),
                Constraint::Length(composer_h),
                Constraint::Length(1),
            ])
            .split(f.area());

        self.viewport_height = chunks[1].height;

        if self.auto_scroll {
            self.scroll = self.max_scroll();
        }

        // Pulse the latest tool dot while the runner is active.
        if self.runner_active {
            let pulse = PULSE_FRAMES[(self.tick / 3) as usize % PULSE_FRAMES.len()];
            if let Some(last) = self.viewport.last_mut() {
                if let Some(first) = last.spans.first_mut() {
                    let stripped = first.content.trim_start();
                    if stripped.starts_with('●')
                        || stripped.starts_with('◉')
                        || stripped.starts_with('○')
                    {
                        first.content = format!("{pulse} ").into();
                    }
                }
            }
        }

        let header_color = if self.runner_active {
            self.theme.accents[(self.tick / 6) as usize % self.theme.accents.len()]
        } else {
            self.theme.header_fg
        };
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                "🦦 Aonyx Agent",
                Style::default()
                    .fg(header_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  project:{}", self.project_slug),
                Style::default().fg(self.theme.dim),
            ),
        ]));
        f.render_widget(header, chunks[0]);

        let viewport = Paragraph::new(Text::from(self.viewport.clone()))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        f.render_widget(viewport, chunks[1]);

        // Suggestions popup (above the composer) — only rendered when active.
        if suggestions_h > 0 {
            let kind_label = match self.suggestion_kind {
                Some(Trigger::At) => "files",
                Some(Trigger::Slash) => "commands",
                None => "",
            };
            let lines: Vec<Line> = self
                .suggestions
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let selected = i == self.suggestion_idx;
                    let marker = if selected { "▸ " } else { "  " };
                    let style = if selected {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(s.clone(), style),
                    ])
                })
                .collect();
            let title = format!(" {} · Tab accept · ↑/↓ navigate · Esc cancel ", kind_label);
            let popup = Paragraph::new(Text::from(lines)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.theme.suggestion_border))
                    .title(title),
            );
            f.render_widget(popup, chunks[2]);
        }

        if self.runner_active {
            let spinner_idx = self.tick as usize % SPINNER_FRAMES.len();
            let spinner = SPINNER_FRAMES[spinner_idx];
            let pulse_color =
                self.theme.accents[(self.tick / 3) as usize % self.theme.accents.len()];
            let blocker = Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("  {spinner} "),
                    Style::default()
                        .fg(pulse_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "runner busy — Esc to quit",
                    Style::default().fg(self.theme.dim),
                ),
            ]))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(pulse_color)),
            );
            f.render_widget(blocker, chunks[3]);
        } else {
            self.composer.set_block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(self.theme.composer_border)),
            );
            f.render_widget(&self.composer, chunks[3]);
        }

        let details = if self.show_tool_details {
            " · details:on"
        } else {
            ""
        };
        let scroll_marker = if self.auto_scroll {
            ""
        } else {
            " · scroll:manual"
        };
        let status_line = if self.runner_active {
            let spinner_idx = self.tick as usize % SPINNER_FRAMES.len();
            let spinner = SPINNER_FRAMES[spinner_idx];
            let spin_color =
                self.theme.accents[(self.tick / 3) as usize % self.theme.accents.len()];
            Line::from(vec![
                Span::styled(
                    format!(" {spinner} "),
                    Style::default().fg(spin_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "{} · {} · turn {} · running{}{} ",
                        self.provider_name, self.model_name, self.turns, details, scroll_marker
                    ),
                    Style::default().fg(self.theme.header_fg),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(" ▸ ", Style::default().fg(self.theme.user_prefix)),
                Span::styled(
                    format!(
                        "{} · {} · turn {} · idle{}{} ",
                        self.provider_name, self.model_name, self.turns, details, scroll_marker
                    ),
                    Style::default().fg(self.theme.status_fg),
                ),
            ])
        };
        let bg = if self.runner_active {
            self.theme.status_busy_bg
        } else {
            self.theme.status_bg
        };
        let status = Paragraph::new(status_line).style(Style::default().bg(bg));
        f.render_widget(status, chunks[4]);
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
    "inline:",
    "  @path/to/file.rs     load the file into the next turn's context",
    "  !ls / !git status    run a shell command locally and feed output back",
    "keys: Shift+Enter newline · ↑/↓ history · PgUp/PgDn scroll · Esc quit",
];

/// Parse `@path` tokens out of the user message.
///
/// Returns the cleaned-up text (with each `@path` re-quoted as ``@path``
/// so the model knows it referenced something) and the list of paths found.
/// A bare `@` with no following non-whitespace is left as-is.
fn extract_refs(input: &str) -> (String, Vec<String>) {
    let mut refs = Vec::new();
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' {
            // Read the path: any non-whitespace following the `@`.
            let mut path = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    break;
                }
                path.push(next);
                chars.next();
            }
            if path.is_empty() {
                out.push('@');
            } else {
                refs.push(path.clone());
                out.push('`');
                out.push('@');
                out.push_str(&path);
                out.push('`');
            }
        } else {
            out.push(c);
        }
    }
    (out, refs)
}

/// Read every `@path` from disk in parallel.
async fn resolve_refs(paths: &[String]) -> Vec<(String, Result<String, String>)> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let result = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| e.to_string());
        out.push((path.clone(), result));
    }
    out
}

/// Build a single system message that lists the resolved files. Returns
/// `None` if every ref failed to read (no point cluttering the transcript).
fn build_refs_message(refs: &[(String, Result<String, String>)]) -> Option<Message> {
    let any_ok = refs.iter().any(|(_, r)| r.is_ok());
    if !any_ok {
        return None;
    }
    let mut content = String::new();
    content.push_str(
        "The user attached the following files (full text follows). Treat them as authoritative context.\n\n",
    );
    for (path, result) in refs {
        match result {
            Ok(text) => {
                content.push_str(&format!("--- {path} ---\n{text}\n\n"));
            }
            Err(e) => {
                content.push_str(&format!("--- {path} ---\n(could not read: {e})\n\n"));
            }
        }
    }
    Some(Message::new(Role::System, content))
}

/// Run a shell command locally and capture combined stdout + stderr.
async fn run_bash(cmd: &str) -> Result<String, String> {
    use tokio::process::Command;
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    let output = command.output().await.map_err(|e| format!("spawn: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut combined = String::new();
    combined.push_str(&stdout);
    if !stderr.is_empty() {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        combined.push_str(&format!("\n[exit {code}]"));
    }
    Ok(combined.trim_end_matches(&['\n', '\r'][..]).to_string())
}

/// Compute the byte offset of `textarea.cursor()` inside `textarea.lines().join("\n")`.
fn cursor_byte_offset(textarea: &TextArea<'_>) -> usize {
    let (row, col) = textarea.cursor();
    let lines = textarea.lines();
    let mut offset = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if i == row {
            offset += line.chars().take(col).map(|c| c.len_utf8()).sum::<usize>();
            return offset;
        }
        offset += line.len() + 1; // +1 for the "\n" join separator
    }
    offset
}

/// Look backward from `cursor` to find a `@` or `/` trigger.
///
/// Returns the trigger kind, its byte position, and the substring between it
/// and the cursor (the active query). Bails out when whitespace is reached
/// before finding a trigger so an `@` mid-sentence does not fire.
fn detect_trigger(text: &str, cursor: usize) -> Option<(Trigger, usize, String)> {
    if cursor == 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut i = cursor;
    while i > 0 {
        i -= 1;
        let c = bytes[i] as char;
        if c == '@' {
            let preceded_by_ws_or_start = i == 0 || (bytes[i - 1] as char).is_whitespace();
            if preceded_by_ws_or_start {
                let query = text[i + 1..cursor].to_string();
                return Some((Trigger::At, i, query));
            }
            return None;
        }
        if c == '/' {
            let at_line_start = i == 0 || bytes[i - 1] == b'\n';
            if at_line_start {
                let query = text[i + 1..cursor].to_string();
                return Some((Trigger::Slash, i, query));
            }
            return None;
        }
        if c.is_whitespace() {
            return None;
        }
    }
    None
}

/// Fuzzy-rank `pool` by `query` using `nucleo-matcher`. Returns up to `limit`
/// best matches in decreasing score order.
fn fuzzy_top(query: &str, pool: &[String], limit: usize) -> Vec<String> {
    use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
    use nucleo_matcher::{Config, Matcher, Utf32Str};

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);

    let mut buf = Vec::new();
    let mut scored: Vec<(String, u32)> = pool
        .iter()
        .filter_map(|s| {
            buf.clear();
            let utf32 = Utf32Str::new(s, &mut buf);
            pattern.score(utf32, &mut matcher).map(|s_| (s.clone(), s_))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.truncate(limit);
    scored.into_iter().map(|(s, _)| s).collect()
}

/// Walk `base` (depth-limited) and return file paths relative to it, using
/// `/` separators. Skips hidden directories (`.git`, `.aonyx`, `target`, …).
fn collect_files(base: &std::path::Path, max_depth: usize, limit: usize) -> Vec<String> {
    use walkdir::WalkDir;
    let mut out = Vec::new();
    for entry in WalkDir::new(base)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git" | ".aonyx" | "target" | "node_modules" | "dist"
            )
        })
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
            if out.len() >= limit {
                break;
            }
        }
    }
    out.sort();
    out
}

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

/// Convert a `ratatui_core::Line` (what `tui-markdown` returns) into a
/// `ratatui::Line<'static>`. `Style`, `Color`, `Modifier` and
/// `HorizontalAlignment` are distinct types between the two crate paths so
/// the conversion goes field-by-field — Markdown colours (headings, code
/// blocks, inline code, bold/italic modifiers) now survive the round-trip.
fn line_to_static(line: ratatui_core::text::Line<'_>) -> Line<'static> {
    let spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|span| Span::styled(span.content.into_owned(), convert_style(span.style)))
        .collect();
    let mut new_line = Line::from(spans);
    new_line.style = convert_style(line.style);
    if let Some(alignment) = line.alignment {
        new_line = new_line.alignment(convert_alignment(alignment));
    }
    new_line
}

fn convert_style(s: ratatui_core::style::Style) -> Style {
    // ratatui_core 0.1 has no `underline_color`; leave it `None`.
    Style {
        fg: s.fg.map(convert_color),
        bg: s.bg.map(convert_color),
        underline_color: None,
        add_modifier: convert_modifier(s.add_modifier),
        sub_modifier: convert_modifier(s.sub_modifier),
    }
}

fn convert_color(c: ratatui_core::style::Color) -> Color {
    use ratatui_core::style::Color as Cc;
    match c {
        Cc::Reset => Color::Reset,
        Cc::Black => Color::Black,
        Cc::Red => Color::Red,
        Cc::Green => Color::Green,
        Cc::Yellow => Color::Yellow,
        Cc::Blue => Color::Blue,
        Cc::Magenta => Color::Magenta,
        Cc::Cyan => Color::Cyan,
        Cc::Gray => Color::Gray,
        Cc::DarkGray => Color::DarkGray,
        Cc::LightRed => Color::LightRed,
        Cc::LightGreen => Color::LightGreen,
        Cc::LightYellow => Color::LightYellow,
        Cc::LightBlue => Color::LightBlue,
        Cc::LightMagenta => Color::LightMagenta,
        Cc::LightCyan => Color::LightCyan,
        Cc::White => Color::White,
        Cc::Rgb(r, g, b) => Color::Rgb(r, g, b),
        Cc::Indexed(i) => Color::Indexed(i),
    }
}

fn convert_modifier(m: ratatui_core::style::Modifier) -> Modifier {
    // Both crates back `Modifier` with the same `bitflags!` bit layout, so
    // the raw bits round-trip safely.
    Modifier::from_bits_truncate(m.bits())
}

fn convert_alignment(a: ratatui_core::layout::HorizontalAlignment) -> Alignment {
    use ratatui_core::layout::HorizontalAlignment as H;
    match a {
        H::Left => Alignment::Left,
        H::Center => Alignment::Center,
        H::Right => Alignment::Right,
    }
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

    #[test]
    fn extract_refs_pulls_paths_and_quotes_them_back() {
        let (cleaned, refs) = extract_refs("look at @src/main.rs and @Cargo.toml together");
        assert_eq!(refs, vec!["src/main.rs", "Cargo.toml"]);
        assert!(cleaned.contains("`@src/main.rs`"));
        assert!(cleaned.contains("`@Cargo.toml`"));
    }

    #[test]
    fn extract_refs_leaves_bare_at_alone() {
        let (cleaned, refs) = extract_refs("send mail @ now");
        assert!(refs.is_empty());
        assert!(cleaned.contains("@ now"));
    }

    #[test]
    fn extract_refs_handles_path_with_dots_and_dashes() {
        let (_, refs) = extract_refs("compare @./crates/aonyx-cli/Cargo.toml please");
        assert_eq!(refs, vec!["./crates/aonyx-cli/Cargo.toml"]);
    }

    #[test]
    fn build_refs_message_skips_when_all_fail() {
        let refs = vec![("missing.rs".to_string(), Err("not found".to_string()))];
        assert!(build_refs_message(&refs).is_none());
    }

    #[test]
    fn build_refs_message_keeps_failures_alongside_successes() {
        let refs = vec![
            ("a.rs".to_string(), Ok("contents".to_string())),
            ("b.rs".to_string(), Err("nope".to_string())),
        ];
        let msg = build_refs_message(&refs).expect("non-empty");
        assert!(msg.content.contains("contents"));
        assert!(msg.content.contains("could not read: nope"));
        assert_eq!(msg.role, Role::System);
    }
}
