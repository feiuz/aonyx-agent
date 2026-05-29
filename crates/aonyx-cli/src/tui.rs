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
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tui_textarea::{CursorMove, TextArea};

use crate::pricing::{self, Pricing};
use crate::session::SlashCommand;
use crate::theme::{self, Theme};

const HISTORY_MAX: usize = 200;
const VIEWPORT_MAX_LINES: usize = 2000;
const MIN_COMPOSER_HEIGHT: u16 = 3;
const MAX_COMPOSER_HEIGHT: u16 = 10;
const SUGGESTION_LIMIT: usize = 8;
const FILE_CACHE_LIMIT: usize = 5000;
const FILE_CACHE_MAX_DEPTH: usize = 8;
/// Per-side line cap for the `fs_write` previewed content (F2). Anything
/// beyond is folded into a dim `(…+N more lines)` marker.
const DIFF_MAX_LINES: usize = 6;

/// Minimum number of characters the streaming assistant text must have
/// grown by since the last Markdown re-render before we re-parse it
/// again (Phase M). Small enough to feel live, large enough that we
/// don't re-render after every single 1-char token from the model.
const STREAM_MD_MIN_INCREMENT: usize = 24;

/// Total line cap for the unified `fs_edit` diff (Phase G). Counts every
/// rendered row regardless of tag; once exceeded, remaining changes
/// collapse into a `(…+N more)` summary.
const UNIFIED_DIFF_MAX_LINES: usize = 18;

/// Context lines emitted around each hunk in the unified `fs_edit` diff
/// (Phase G).
const UNIFIED_DIFF_CONTEXT: usize = 1;

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
    "/themes",
    "/vim",
    "/undo",
    "/find",
    "/load",
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

/// What the user is currently typing into the composer (Phase I).
///
/// Drives the inline syntax-highlight: the whole composer text + border
/// adopt a colour appropriate to the mode so the user sees what kind of
/// action `Enter` will dispatch *before* hitting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerMode {
    /// Regular chat message → default theme.
    Chat,
    /// First non-empty line starts with `/` → slash command.
    Slash,
    /// First non-empty line starts with `!` → inline bash.
    Bash,
}

/// Vim-style editing mode (F3). Toggle the whole feature on/off with
/// `/vim`; once on, `Esc` enters Normal mode and `i`/`a` returns to
/// Insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VimMode {
    /// Vim mode is off (default). `Esc` quits the session.
    Off,
    /// Inside vim mode, composer captures keys. `Esc` enters Normal.
    Insert,
    /// Inside vim mode, keys drive the viewport. `i`/`a` returns to
    /// Insert. `j`/`k` scroll, `g`/`G` top/bottom, `q` quits.
    Normal,
}

impl VimMode {
    fn label(self) -> Option<&'static str> {
        match self {
            VimMode::Off => None,
            VimMode::Insert => Some("INS"),
            VimMode::Normal => Some("NRM"),
        }
    }
}

/// Frames for the running-runner spinner. Braille dots feel lively without
/// burning CPU on the redraw.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Pulse frames for in-flight tool dots — small to keep eye-strain low.
const PULSE_FRAMES: &[&str] = &["●", "◉", "○", "◉"];

/// What happens when the user accepts a palette entry. Carries enough
/// information to be dispatched without re-parsing anything.
#[derive(Debug, Clone)]
enum PaletteAction {
    /// Run an existing slash command.
    Slash(SlashCommand),
    /// Switch to a bundled theme by name.
    SwitchTheme(String),
}

/// One row in the Ctrl+P palette.
#[derive(Debug, Clone)]
struct PaletteEntry {
    /// Bold label shown on the left.
    label: String,
    /// Dim hint shown on the right.
    hint: String,
    /// What to dispatch when accepted.
    action: PaletteAction,
}

/// Floating Ctrl+P command palette state.
#[derive(Debug)]
struct Palette {
    /// `true` while the overlay is visible.
    open: bool,
    /// User-typed filter.
    query: String,
    /// Static list of every action surfaced to the palette.
    entries: Vec<PaletteEntry>,
    /// Indices into `entries` matching `query` (ranked by score).
    filtered: Vec<usize>,
    /// Index inside `filtered` of the highlighted row.
    selected: usize,
}

impl Palette {
    fn new() -> Self {
        let entries = build_palette_entries();
        let filtered = (0..entries.len()).collect();
        Self {
            open: false,
            query: String::new(),
            entries,
            filtered,
            selected: 0,
        }
    }

    fn show(&mut self) {
        self.open = true;
        self.query.clear();
        self.filtered = (0..self.entries.len()).collect();
        self.selected = 0;
    }

    fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }

    fn refilter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let labels: Vec<String> = self
                .entries
                .iter()
                .map(|e| format!("{} {}", e.label, e.hint))
                .collect();
            self.filtered = fuzzy_top_idx(&self.query, &labels, self.entries.len());
        }
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    fn current(&self) -> Option<&PaletteEntry> {
        self.filtered
            .get(self.selected)
            .and_then(|i| self.entries.get(*i))
    }
}

/// Static catalogue of palette actions. Order = default sort when no query.
fn build_palette_entries() -> Vec<PaletteEntry> {
    let mut out = vec![
        PaletteEntry {
            label: "/new".into(),
            hint: "Start a fresh conversation".into(),
            action: PaletteAction::Slash(SlashCommand::New),
        },
        PaletteEntry {
            label: "/help".into(),
            hint: "Show every command".into(),
            action: PaletteAction::Slash(SlashCommand::Help),
        },
        PaletteEntry {
            label: "/models".into(),
            hint: "Active provider + model".into(),
            action: PaletteAction::Slash(SlashCommand::Models),
        },
        PaletteEntry {
            label: "/sessions".into(),
            hint: "List sessions for this project".into(),
            action: PaletteAction::Slash(SlashCommand::Sessions),
        },
        PaletteEntry {
            label: "/export".into(),
            hint: "Export conversation to Markdown".into(),
            action: PaletteAction::Slash(SlashCommand::Export(None)),
        },
        PaletteEntry {
            label: "/details".into(),
            hint: "Toggle verbose tool output".into(),
            action: PaletteAction::Slash(SlashCommand::Details),
        },
        PaletteEntry {
            label: "/thinking".into(),
            hint: "Toggle reasoning visibility".into(),
            action: PaletteAction::Slash(SlashCommand::Thinking),
        },
        PaletteEntry {
            label: "/editor".into(),
            hint: "Open $EDITOR (legacy mode)".into(),
            action: PaletteAction::Slash(SlashCommand::Editor),
        },
        PaletteEntry {
            label: "/init".into(),
            hint: "Drop agent.yaml in project root".into(),
            action: PaletteAction::Slash(SlashCommand::Init),
        },
        PaletteEntry {
            label: "/vim".into(),
            hint: "Toggle vim-style modal editing".into(),
            action: PaletteAction::Slash(SlashCommand::Vim),
        },
        PaletteEntry {
            label: "/undo".into(),
            hint: "Revert the last fs_edit / fs_write".into(),
            action: PaletteAction::Slash(SlashCommand::Undo),
        },
        PaletteEntry {
            label: "/find".into(),
            hint: "Search past sessions (needs a query: /find oauth)".into(),
            action: PaletteAction::Slash(SlashCommand::Find(None)),
        },
        PaletteEntry {
            label: "/load".into(),
            hint: "Switch to a session by id prefix".into(),
            action: PaletteAction::Slash(SlashCommand::Load(None)),
        },
        PaletteEntry {
            label: "/quit".into(),
            hint: "Exit Aonyx".into(),
            action: PaletteAction::Slash(SlashCommand::Quit),
        },
    ];
    for name in theme::available_names() {
        out.push(PaletteEntry {
            label: format!("Theme: {name}"),
            hint: format!("Switch palette to {name}"),
            action: PaletteAction::SwitchTheme(name.to_string()),
        });
    }
    out
}

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
    // Cache pricing once — provider + model can't change mid-session.
    let cached_pricing = pricing::lookup(&provider_name, &model);

    let mut app = TuiApp {
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
        last_md_render_chars: 0,
        suggestions: Vec::new(),
        suggestion_idx: 0,
        suggestion_kind: None,
        suggestion_trigger_pos: 0,
        file_cache: None,
        turn_started_at: None,
        palette: Palette::new(),
        vim_mode: VimMode::Off,
        viewport_rect: None,
        palette_results_rect: None,
        total_input_tokens: 0,
        total_output_tokens: 0,
        pricing: cached_pricing,
        quit: false,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Apply the initial composer styling (Phase I) so the border picks
    // up the active theme straight away instead of staying on the
    // bootstrap DarkGray.
    app.apply_composer_style();

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
    /// Char count of `current_assistant_text` at the last Markdown
    /// re-render — throttles live re-rendering during streaming
    /// (Phase M).
    last_md_render_chars: usize,

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

    /// Floating Ctrl+P palette (F1).
    palette: Palette,

    /// Vim editing mode toggle (F3). Off by default.
    vim_mode: VimMode,

    /// Last drawn rectangle of the conversation viewport — used by
    /// mouse-wheel routing (Phase H).
    viewport_rect: Option<Rect>,
    /// Last drawn rectangle of the palette results pane — used to map a
    /// click to the row index (Phase H).
    palette_results_rect: Option<Rect>,

    /// Cumulative input tokens estimated for this session (Phase K).
    total_input_tokens: u64,
    /// Cumulative output tokens estimated for this session (Phase K).
    total_output_tokens: u64,
    /// Cached pricing for the active provider+model, looked up once at
    /// startup. `None` for local / free providers (Phase K).
    pricing: Option<Pricing>,

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
                    Event::Mouse(m) => {
                        self.handle_mouse(m).await;
                    }
                    Event::Resize(_, _) => { /* ratatui re-renders next frame */ }
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
    /// Render whatever's accumulated in `current_assistant_text` as
    /// Markdown, replacing any previously-rendered lines for the same
    /// message in place.
    ///
    /// Called both during streaming (every delta, Phase M) and at
    /// `AssistantMessageEnd`. Idempotent — re-running it after the same
    /// buffer is a no-op visually.
    fn rerender_assistant_markdown(&mut self) {
        let Some(start) = self.assistant_msg_start else {
            return;
        };
        if self.current_assistant_text.trim().is_empty() {
            return;
        }
        if start > self.viewport.len() {
            return;
        }

        // Drop the previously-rendered lines for this message.
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
        self.last_md_render_chars = self.current_assistant_text.chars().count();
    }

    /// Decide whether the just-arrived `delta` warrants a Markdown
    /// re-render, or whether it's small enough to wait for more text
    /// before re-parsing (Phase M).
    fn should_rerender_markdown(&self, delta: &str) -> bool {
        // First chunk of a fresh message — always render so the user
        // sees output immediately instead of staring at the thinking
        // placeholder until 24 chars accumulate.
        if self.last_md_render_chars == 0 && !self.current_assistant_text.is_empty() {
            return true;
        }
        // Newlines often complete a block (paragraph / heading / list
        // item / code fence), so always re-render then — that's when
        // Markdown structure becomes parseable.
        if delta.contains('\n') {
            return true;
        }
        let new_chars = self.current_assistant_text.chars().count();
        new_chars.saturating_sub(self.last_md_render_chars) >= STREAM_MD_MIN_INCREMENT
    }

    /// Backwards-compatible alias kept for the AssistantMessageEnd path.
    fn finalize_assistant_message(&mut self) {
        self.rerender_assistant_markdown();
    }

    fn apply_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::AssistantDelta(text) => {
                if !self.first_delta_received {
                    self.retire_thinking_line();
                    self.first_delta_received = true;
                    // Remember where this assistant message starts so we
                    // can re-render the Markdown in place as the model
                    // streams (Phase M).
                    self.assistant_msg_start = Some(self.viewport.len());
                }
                // Phase K — accumulate output tokens live as the model
                // streams.
                self.total_output_tokens = self
                    .total_output_tokens
                    .saturating_add(pricing::estimate_tokens(&text));
                self.current_assistant_text.push_str(&text);
                // Phase M — re-render Markdown live so headings / bold
                // / code fences light up while the model is still
                // typing. Throttled by `should_rerender_markdown` so
                // single-char tokens don't pin the CPU; the running
                // text always rests on a fully-rendered snapshot.
                if self.should_rerender_markdown(&text) {
                    self.rerender_assistant_markdown();
                }
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
                self.last_md_render_chars = 0;
            }
            TurnEvent::ToolStart { name, args, class } => {
                self.retire_thinking_line();
                self.first_delta_received = true;
                let dot_color = match class {
                    SafetyClass::Safe => Color::Cyan,
                    SafetyClass::Caution => Color::Yellow,
                    SafetyClass::Destructive => Color::Red,
                };
                // For fs_edit / fs_write the abbreviated-args preview is
                // useless (huge content blobs). Render a colored diff
                // preview underneath instead — F2.
                let is_diff_tool = name == "fs_edit" || name == "fs_write";
                let preview = if is_diff_tool {
                    args.get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string()
                } else {
                    abbreviate_value(&args, 80)
                };
                self.push_line(Line::from(vec![
                    Span::styled("● ", Style::default().fg(dot_color)),
                    Span::styled(name.clone(), Style::default().fg(Color::Cyan)),
                    Span::styled(format!("({preview})"), Style::default().fg(Color::DarkGray)),
                ]));
                if is_diff_tool {
                    self.push_diff_preview(&name, &args);
                }
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

        // Palette swallows every key while open. Handled first so Ctrl+C/Esc
        // don't quit the session by accident when the palette is showing.
        if self.palette.open {
            self.handle_palette_key(key).await;
            return;
        }

        // Vim Normal mode (F3) — composer is parked; keys drive the viewport.
        if self.vim_mode == VimMode::Normal {
            self.handle_vim_normal_key(key);
            return;
        }

        match key.code {
            // Ctrl+P opens the floating command palette (F1).
            Char('p') if ctrl => {
                self.palette.show();
            }
            // While the suggestions popup is open, Esc just closes it.
            Esc if suggestions_open => {
                self.dismiss_suggestions();
            }
            // In vim Insert mode, Esc enters Normal instead of quitting.
            Esc if self.vim_mode == VimMode::Insert => {
                self.vim_mode = VimMode::Normal;
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
        // Phase I — refresh the composer's text + border colour now that
        // the input may have shifted between Chat / Slash / Bash modes.
        self.apply_composer_style();
    }

    /// Recolour the composer's text + border based on the detected
    /// [`ComposerMode`] (Phase I).
    ///
    /// * Chat → default theme (no extra bold, theme border).
    /// * Slash → magenta bold + magenta border.
    /// * Bash → yellow bold + yellow border.
    fn apply_composer_style(&mut self) {
        let mode = detect_composer_mode(&self.composer);
        let (text_style, border_color) = match mode {
            ComposerMode::Chat => (
                Style::default().fg(self.theme.header_fg),
                self.theme.composer_border,
            ),
            ComposerMode::Slash => (
                Style::default()
                    .fg(self.theme.suggestion_border)
                    .add_modifier(Modifier::BOLD),
                self.theme.suggestion_border,
            ),
            ComposerMode::Bash => (
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                Color::Yellow,
            ),
        };
        self.composer.set_style(text_style);
        self.composer.set_block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(border_color)),
        );
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
        // Block + text style are re-applied by `apply_composer_style` —
        // bootstrap with a sensible default for the cursor + placeholder.
        next.set_cursor_line_style(Style::default());
        next.set_placeholder_text("type a message — Enter to send, Shift+Enter for newline");
        next.move_cursor(CursorMove::Bottom);
        next.move_cursor(CursorMove::End);
        self.composer = next;
        self.apply_composer_style();
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
        // Phase K — pre-flight estimate of the input tokens the runner
        // is about to send. The agent loop may grow the messages list
        // with tool results before sending again, but charging once at
        // turn start is a sane approximation of the first request.
        let input_estimate: u64 = messages
            .iter()
            .map(|m| pricing::estimate_tokens(&m.content))
            .sum();
        self.total_input_tokens = self.total_input_tokens.saturating_add(input_estimate);
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
            SlashCommand::Vim => {
                self.vim_mode = match self.vim_mode {
                    VimMode::Off => {
                        self.push_dim(
                            "vim mode: on (Esc = Normal · i/a = Insert · j/k scroll · g/G top/bottom · q quit)",
                        );
                        VimMode::Insert
                    }
                    VimMode::Insert | VimMode::Normal => {
                        self.push_dim("vim mode: off");
                        VimMode::Off
                    }
                };
            }
            SlashCommand::Find(target) => {
                let Some(query) = target.filter(|q| !q.trim().is_empty()) else {
                    self.push_dim("usage: /find <query> — searches all sessions");
                    return;
                };
                match self.session_store.search(query.trim(), 10).await {
                    Ok(hits) if hits.is_empty() => self.push_dim(&format!(
                        "no hits for '{}' across {} project(s)",
                        query.trim(),
                        "all"
                    )),
                    Ok(hits) => {
                        self.push_dim(&format!(
                            "{} hit(s) for '{}' — `/load <id>` to switch:",
                            hits.len(),
                            query.trim()
                        ));
                        for h in hits {
                            let short_id: String =
                                h.id.to_string().chars().take(8).collect();
                            let header = format!(
                                "  [{short_id}] {} · {} · {} turn(s) · \"{}\"",
                                h.updated_at.format("%Y-%m-%d %H:%M"),
                                h.project,
                                h.turns,
                                h.title
                            );
                            self.push_dim(&header);
                            self.push_dim(&format!("    └ {}", h.snippet));
                        }
                    }
                    Err(e) => self.push_line(error_line(format!("search failed: {e}"))),
                }
            }
            SlashCommand::Load(target) => {
                let Some(prefix) = target.filter(|q| !q.trim().is_empty()) else {
                    self.push_dim("usage: /load <id-prefix> — from a /find result");
                    return;
                };
                match self
                    .session_store
                    .find_by_id_prefix(prefix.trim(), 5)
                    .await
                {
                    Ok(matches) if matches.is_empty() => self
                        .push_dim(&format!("no session matches prefix '{}'", prefix.trim())),
                    Ok(matches) if matches.len() > 1 => {
                        self.push_dim(&format!(
                            "ambiguous prefix '{}' — {} matches:",
                            prefix.trim(),
                            matches.len()
                        ));
                        for r in matches {
                            let short: String = r.id.to_string().chars().take(8).collect();
                            self.push_dim(&format!(
                                "  [{short}] {} · {}",
                                r.updated_at.format("%Y-%m-%d %H:%M"),
                                r.title
                            ));
                        }
                    }
                    Ok(mut matches) => {
                        let target_record = matches.remove(0);
                        // Persist current session before swapping so we
                        // don't lose in-flight turns.
                        let _ = self
                            .session_store
                            .update(self.session_id, self.messages.clone(), self.turns)
                            .await;
                        // Swap in the loaded session's state.
                        let loaded_id = target_record.id;
                        let short: String =
                            loaded_id.to_string().chars().take(8).collect();
                        self.session_id = loaded_id;
                        self.messages = target_record.messages;
                        self.turns = target_record.turns;
                        self.project_slug = target_record.project.clone();
                        self.viewport.clear();
                        self.viewport.push(Line::from(Span::styled(
                            format!(
                                "🦦 loaded session [{short}] · {} · \"{}\"",
                                target_record.project, target_record.title
                            ),
                            Style::default().fg(self.theme.dim),
                        )));
                        self.auto_scroll = true;
                        self.scroll = 0;
                    }
                    Err(e) => self.push_line(error_line(format!("load failed: {e}"))),
                }
            }
            SlashCommand::Undo => match aonyx_tools::undo::pop_last_snapshot() {
                Ok(Some(snap)) => match aonyx_tools::undo::restore(&snap) {
                    Ok(()) => {
                        let detail = if snap.prior.is_none() {
                            "deleted file"
                        } else {
                            "restored prior content"
                        };
                        self.push_dim(&format!(
                            "undo: {} ({}) — {detail}",
                            snap.path, snap.tool
                        ));
                    }
                    Err(e) => self.push_line(error_line(format!("undo failed: {e}"))),
                },
                Ok(None) => self.push_dim("undo: nothing to revert"),
                Err(e) => self.push_line(error_line(format!("undo failed: {e}"))),
            },
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

    /// Drive the floating Ctrl+P palette: typing filters, ↑/↓ navigate,
    /// Enter accepts, Esc / Ctrl+P closes.
    async fn handle_palette_key(&mut self, key: KeyEvent) {
        use KeyCode::*;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            Esc => self.palette.close(),
            Char('p') if ctrl => self.palette.close(),
            Char('c') | Char('d') if ctrl => {
                self.palette.close();
                self.quit = true;
            }
            Up => self.palette.move_up(),
            Down => self.palette.move_down(),
            Enter => {
                let action = self.palette.current().map(|e| e.action.clone());
                self.palette.close();
                if let Some(action) = action {
                    self.dispatch_palette_action(action).await;
                }
            }
            Backspace => {
                self.palette.query.pop();
                self.palette.refilter();
            }
            Char(c) if !ctrl => {
                self.palette.query.push(c);
                self.palette.refilter();
            }
            _ => {}
        }
    }

    /// Route a mouse event (Phase H).
    ///
    /// * Scroll wheel — always scrolls the viewport, 3 lines per tick.
    ///   `ScrollDown` re-arms `auto_scroll` if it reaches the bottom.
    /// * Left click inside the palette results pane — selects the
    ///   corresponding row and dispatches it (single-click accept, like
    ///   VS Code's Cmd+P). Clicks outside the palette close it.
    async fn handle_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollUp => {
                self.auto_scroll = false;
                self.scroll = self.scroll.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => {
                self.scroll = self.scroll.saturating_add(3);
                self.clamp_scroll_and_maybe_resume_auto();
            }
            MouseEventKind::Down(MouseButton::Left) if self.palette.open => {
                if let Some(rect) = self.palette_results_rect {
                    if rect_contains(rect, m.column, m.row) {
                        let row_in_pane = m.row.saturating_sub(rect.y) as usize;
                        // The results pane scrolls so the selected row sits
                        // within the visible window — mirror that math to
                        // map a y-offset back to a `filtered` index.
                        let max_rows = rect.height as usize;
                        let scroll = self
                            .palette
                            .selected
                            .saturating_sub(max_rows.saturating_sub(1));
                        let target = scroll + row_in_pane;
                        if target < self.palette.filtered.len() {
                            self.palette.selected = target;
                            let action = self.palette.current().map(|e| e.action.clone());
                            self.palette.close();
                            if let Some(action) = action {
                                self.dispatch_palette_action(action).await;
                            }
                        }
                    } else {
                        // Clicking outside the palette dismisses it.
                        self.palette.close();
                    }
                }
            }
            _ => {}
        }
    }

    /// Drive vim Normal mode (F3): viewport navigation while the composer
    /// is parked. `i`/`a` returns to Insert, `q` quits the session.
    fn handle_vim_normal_key(&mut self, key: KeyEvent) {
        use KeyCode::*;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Always-respected exits.
            Char('c') | Char('d') if ctrl => self.quit = true,
            Char('q') => self.quit = true,
            // Back to Insert.
            Char('i') | Char('a') | Char('o') => self.vim_mode = VimMode::Insert,
            // Scrolling.
            Char('j') | Down => {
                self.scroll = self.scroll.saturating_add(1);
                self.clamp_scroll_and_maybe_resume_auto();
            }
            Char('k') | Up => {
                self.auto_scroll = false;
                self.scroll = self.scroll.saturating_sub(1);
            }
            Char('g') | Home => {
                self.auto_scroll = false;
                self.scroll = 0;
            }
            Char('G') | End => {
                self.auto_scroll = true;
            }
            PageUp => {
                self.auto_scroll = false;
                self.scroll = self.scroll.saturating_sub(8);
            }
            PageDown => {
                self.scroll = self.scroll.saturating_add(8);
                self.clamp_scroll_and_maybe_resume_auto();
            }
            _ => {}
        }
    }

    /// Execute a `PaletteAction` exactly as if the user had typed the
    /// equivalent slash command.
    async fn dispatch_palette_action(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::Slash(cmd) => self.handle_slash(cmd).await,
            PaletteAction::SwitchTheme(name) => {
                self.handle_slash(SlashCommand::Themes(Some(name))).await;
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

    /// Build the `· ~Xk tok · ~$Y.YY` suffix for the status bar.
    ///
    /// Stays empty until at least one turn has produced tokens — no
    /// point staring at `0 tok · <$0.01` during the opening prompt.
    /// Cost is omitted for providers without pricing (local + claude-
    /// code).
    fn cost_marker_string(&self) -> String {
        let total = self.total_input_tokens + self.total_output_tokens;
        if total == 0 {
            return String::new();
        }
        let tokens = pricing::format_tokens(total);
        match self.pricing {
            Some(p) => {
                let cost = pricing::estimate_cost(
                    p,
                    self.total_input_tokens,
                    self.total_output_tokens,
                );
                format!(" · ~{tokens} tok · ~{}", pricing::format_cost(cost))
            }
            None => format!(" · ~{tokens} tok"),
        }
    }

    /// Render a unified-style diff preview underneath an `fs_edit` /
    /// `fs_write` ToolStart line. F2.
    ///
    /// * `fs_edit` shows the old block in red (`-`) followed by the new
    ///   block in green (`+`).
    /// * `fs_write` shows the new content in green (`+`) since there is
    ///   no in-flight "before" snapshot.
    ///
    /// Long blocks are clipped at [`DIFF_MAX_LINES`] with a dim `(…+N
    /// more)` marker so a 500-line rewrite doesn't flood the viewport.
    fn push_diff_preview(&mut self, name: &str, args: &serde_json::Value) {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        let header_style = Style::default()
            .fg(self.theme.assistant_prefix)
            .add_modifier(Modifier::BOLD);
        let frame_style = Style::default().fg(self.theme.dim);
        self.push_line(Line::from(vec![
            Span::styled("  ┌─ ", frame_style),
            Span::styled(format!("{name} · {path}"), header_style),
        ]));
        match name {
            "fs_edit" => {
                let old = args
                    .get("old_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let new = args
                    .get("new_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.push_unified_diff(old, new);
            }
            "fs_write" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                self.push_diff_lines("+ ", content, Color::Green);
            }
            _ => {}
        }
        self.push_line(Line::from(Span::styled("  └─", frame_style)));
    }

    fn push_diff_lines(&mut self, prefix: &'static str, text: &str, color: Color) {
        let frame_style = Style::default().fg(self.theme.dim);
        let lines: Vec<&str> = text.lines().collect();
        let take = lines.len().min(DIFF_MAX_LINES);
        for line in lines.iter().take(take) {
            self.push_line(Line::from(vec![
                Span::styled("  │ ", frame_style),
                Span::styled(prefix, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(line.to_string(), Style::default().fg(color)),
            ]));
        }
        if lines.len() > DIFF_MAX_LINES {
            let omitted = lines.len() - DIFF_MAX_LINES;
            self.push_line(Line::from(vec![
                Span::styled("  │ ", frame_style),
                Span::styled(
                    format!("… (+{omitted} more line{})", if omitted == 1 { "" } else { "s" }),
                    Style::default().fg(self.theme.dim).add_modifier(Modifier::ITALIC),
                ),
            ]));
        }
    }

    /// Render a unified diff between `old` and `new` (Phase G).
    ///
    /// Groups changes into hunks with [`UNIFIED_DIFF_CONTEXT`] context lines
    /// each, separated by a dim `…` marker. Lines are tagged `-` (red),
    /// `+` (green), or ` ` (dim context). Truncates at
    /// [`UNIFIED_DIFF_MAX_LINES`] with a trailing `(+N more)` summary so a
    /// 200-line refactor doesn't flood the viewport.
    fn push_unified_diff(&mut self, old: &str, new: &str) {
        use similar::{ChangeTag, TextDiff};

        let frame_style = Style::default().fg(self.theme.dim);
        let diff = TextDiff::from_lines(old, new);
        let groups = diff.grouped_ops(UNIFIED_DIFF_CONTEXT);

        if groups.is_empty() {
            self.push_line(Line::from(vec![
                Span::styled("  │ ", frame_style),
                Span::styled(
                    "(no change)",
                    Style::default()
                        .fg(self.theme.dim)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
            return;
        }

        let mut emitted = 0usize;
        let mut truncated = 0usize;

        for (i, group) in groups.iter().enumerate() {
            if i > 0 && emitted < UNIFIED_DIFF_MAX_LINES {
                self.push_line(Line::from(vec![
                    Span::styled("  │ ", frame_style),
                    Span::styled(
                        "  …",
                        Style::default()
                            .fg(self.theme.dim)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
                emitted += 1;
            }
            for op in group {
                for change in diff.iter_changes(op) {
                    if emitted >= UNIFIED_DIFF_MAX_LINES {
                        truncated += 1;
                        continue;
                    }
                    let (prefix, color, bold) = match change.tag() {
                        ChangeTag::Delete => ("- ", Color::Red, true),
                        ChangeTag::Insert => ("+ ", Color::Green, true),
                        ChangeTag::Equal => ("  ", self.theme.dim, false),
                    };
                    let text = change.to_string();
                    let text = text.trim_end_matches(['\n', '\r']);
                    let mut style = Style::default().fg(color);
                    if bold {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    self.push_line(Line::from(vec![
                        Span::styled("  │ ", frame_style),
                        Span::styled(prefix, style),
                        Span::styled(text.to_string(), Style::default().fg(color)),
                    ]));
                    emitted += 1;
                }
            }
        }

        if truncated > 0 {
            self.push_line(Line::from(vec![
                Span::styled("  │ ", frame_style),
                Span::styled(
                    format!(
                        "… (+{truncated} more change{})",
                        if truncated == 1 { "" } else { "s" }
                    ),
                    Style::default()
                        .fg(self.theme.dim)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }
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
        self.viewport_rect = Some(chunks[1]);

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
            // Phase I: the composer's border + text style are owned by
            // `apply_composer_style()` — re-setting the block here would
            // clobber slash/bash highlighting on every redraw.
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
        let vim_marker = match self.vim_mode.label() {
            Some(tag) => format!(" · vim:{tag}"),
            None => String::new(),
        };
        // Phase K — token + cost indicator. Cost only shown when we
        // have a price for this provider/model.
        let cost_marker = self.cost_marker_string();
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
                        "{} · {} · turn {} · running{}{}{}{} ",
                        self.provider_name,
                        self.model_name,
                        self.turns,
                        details,
                        scroll_marker,
                        vim_marker,
                        cost_marker
                    ),
                    Style::default().fg(self.theme.header_fg),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(" ▸ ", Style::default().fg(self.theme.user_prefix)),
                Span::styled(
                    format!(
                        "{} · {} · turn {} · idle{}{}{}{} ",
                        self.provider_name,
                        self.model_name,
                        self.turns,
                        details,
                        scroll_marker,
                        vim_marker,
                        cost_marker
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

        // Palette floats on top of everything else — rendered last so it
        // wins the z-order.
        if self.palette.open {
            self.render_palette(f);
        }
    }

    /// Draw the floating Ctrl+P command palette centered on screen.
    fn render_palette(&mut self, f: &mut Frame<'_>) {
        let area = f.area();
        // Centered 60% wide × 50% tall, clamped so it fits small terminals.
        let width = (area.width as u32 * 60 / 100).clamp(40, 90) as u16;
        let height = (area.height as u32 * 50 / 100).clamp(8, 20) as u16;
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup = ratatui::layout::Rect::new(x, y, width, height);

        // Clear the underlying region so transparent borders don't leak.
        f.render_widget(ratatui::widgets::Clear, popup);

        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(popup);

        // Query input on top.
        let query_text = if self.palette.query.is_empty() {
            "type to filter…".to_string()
        } else {
            self.palette.query.clone()
        };
        let query_style = if self.palette.query.is_empty() {
            Style::default().fg(self.theme.dim)
        } else {
            Style::default()
                .fg(self.theme.header_fg)
                .add_modifier(Modifier::BOLD)
        };
        let count_label = format!(
            " {} / {} ",
            self.palette.filtered.len(),
            self.palette.entries.len()
        );
        let query = Paragraph::new(Line::from(vec![
            Span::styled("  › ", Style::default().fg(self.theme.user_prefix)),
            Span::styled(query_text, query_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.suggestion_border))
                .title(" Ctrl+P · Command palette ")
                .title_alignment(Alignment::Left)
                .title_bottom(Line::from(Span::styled(
                    count_label,
                    Style::default().fg(self.theme.dim),
                )))
                .title_alignment(Alignment::Left),
        );
        f.render_widget(query, inner[0]);

        // Results.
        let max_rows = inner[1].height.saturating_sub(2) as usize;
        let total = self.palette.filtered.len();
        let scroll = self.palette.selected.saturating_sub(max_rows.saturating_sub(1));
        let visible_end = (scroll + max_rows).min(total);
        let visible = &self.palette.filtered[scroll..visible_end];

        let lines: Vec<Line> = if visible.is_empty() {
            vec![Line::from(Span::styled(
                "  (no match)",
                Style::default().fg(self.theme.dim),
            ))]
        } else {
            visible
                .iter()
                .enumerate()
                .map(|(i, idx)| {
                    let entry = &self.palette.entries[*idx];
                    let selected = scroll + i == self.palette.selected;
                    let marker = if selected { "▸ " } else { "  " };
                    let label_style = if selected {
                        Style::default()
                            .fg(self.theme.assistant_prefix)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(self.theme.header_fg)
                    };
                    let hint_style = Style::default().fg(self.theme.dim);
                    Line::from(vec![
                        Span::styled(marker, label_style),
                        Span::styled(entry.label.clone(), label_style),
                        Span::raw("  "),
                        Span::styled(entry.hint.clone(), hint_style),
                    ])
                })
                .collect()
        };
        let footer = Line::from(Span::styled(
            " ↑/↓ navigate · Enter accept · Esc close ",
            Style::default().fg(self.theme.dim),
        ));
        let results = Paragraph::new(Text::from(lines)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.suggestion_border))
                .title_bottom(footer),
        );
        f.render_widget(results, inner[1]);
        // Cache rects for mouse hit-testing (Phase H). The results area is
        // the inner content of the block — strip 1 cell on each side for
        // the border.
        self.palette_results_rect = Some(rect_shrink(inner[1], 1));
    }
}

/// Shrink `r` by `n` cells on every side, clamped to zero. Used to map a
/// `Block`-bordered widget back to its content area for mouse hit-testing.
fn rect_shrink(r: Rect, n: u16) -> Rect {
    let x = r.x.saturating_add(n);
    let y = r.y.saturating_add(n);
    let width = r.width.saturating_sub(n.saturating_mul(2));
    let height = r.height.saturating_sub(n.saturating_mul(2));
    Rect::new(x, y, width, height)
}

/// Return `true` when `(x, y)` falls inside the rectangle.
fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Classify whatever the user is typing into the composer (Phase I).
///
/// Inspects the first non-empty line; if it starts with `/` it's a
/// slash command, `!` it's inline bash, otherwise a regular chat
/// message. `@path` references inside a chat message stay `Chat` —
/// they're recognised separately by the suggestion popup.
fn detect_composer_mode(textarea: &TextArea<'_>) -> ComposerMode {
    let first = textarea
        .lines()
        .iter()
        .find(|l| !l.trim().is_empty())
        .cloned()
        .unwrap_or_default();
    let t = first.trim_start();
    if t.starts_with('/') {
        ComposerMode::Slash
    } else if t.starts_with('!') {
        ComposerMode::Bash
    } else {
        ComposerMode::Chat
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
    "  /themes /t [name]    switch palette (default, catppuccin, dracula, gruvbox)",
    "  /vim                 toggle vim modal editing (F3)",
    "  /undo /u             revert last fs_edit / fs_write (Phase J)",
    "  /find /f <query>     search past sessions across every project (Phase L)",
    "  /load /switch <id>   switch to a session by id prefix (Phase L)",
    "  /editor /e           legacy-mode only for now",
    "  /init                drop an agent.yaml in the project root",
    "inline:",
    "  @path/to/file.rs     load the file into the next turn's context",
    "  !ls / !git status    run a shell command locally and feed output back",
    "keys: Ctrl+P palette · Shift+Enter newline · ↑/↓ history · PgUp/PgDn scroll · Esc quit",
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

/// Same as [`fuzzy_top`] but returns the indices of matching `pool` entries,
/// ordered by descending score. Used by the Ctrl+P palette where the entry
/// list is fixed and we need to map back to the source struct.
fn fuzzy_top_idx(query: &str, pool: &[String], limit: usize) -> Vec<usize> {
    use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
    use nucleo_matcher::{Config, Matcher, Utf32Str};

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);

    let mut buf = Vec::new();
    let mut scored: Vec<(usize, u32)> = pool
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            buf.clear();
            let utf32 = Utf32Str::new(s, &mut buf);
            pattern.score(utf32, &mut matcher).map(|sc| (i, sc))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.truncate(limit);
    scored.into_iter().map(|(i, _)| i).collect()
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
    fn palette_initially_lists_every_entry() {
        let p = Palette::new();
        assert!(!p.open);
        assert_eq!(p.filtered.len(), p.entries.len());
        assert!(p.entries.len() >= 10);
    }

    #[test]
    fn palette_refilter_narrows_by_query() {
        let mut p = Palette::new();
        let total = p.entries.len();
        p.query = "themes".into();
        p.refilter();
        assert!(p.filtered.len() < total);
        assert!(p.filtered.len() >= 1);
    }

    #[test]
    fn palette_refilter_no_match_clamps_selected_to_zero() {
        let mut p = Palette::new();
        p.selected = 5;
        p.query = "zzzzz_no_match_xxxx".into();
        p.refilter();
        assert_eq!(p.filtered.len(), 0);
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn detect_composer_mode_classifies_first_non_empty_line() {
        let chat = TextArea::from(["", "  ", "hello world"]);
        let slash = TextArea::from(["", "/help"]);
        let slash_indented = TextArea::from(["", "   /themes dracula"]);
        let bash = TextArea::from(["!ls -la"]);
        let bare_at = TextArea::from(["@README.md what is this"]);
        assert_eq!(detect_composer_mode(&chat), ComposerMode::Chat);
        assert_eq!(detect_composer_mode(&slash), ComposerMode::Slash);
        assert_eq!(detect_composer_mode(&slash_indented), ComposerMode::Slash);
        assert_eq!(detect_composer_mode(&bash), ComposerMode::Bash);
        // `@` refs live inside Chat — they're surfaced by the suggestion popup.
        assert_eq!(detect_composer_mode(&bare_at), ComposerMode::Chat);
    }

    #[test]
    fn rect_contains_inclusive_on_low_corner_exclusive_on_high() {
        let r = Rect::new(10, 5, 4, 3); // covers x in [10,14), y in [5,8)
        assert!(rect_contains(r, 10, 5));
        assert!(rect_contains(r, 13, 7));
        assert!(!rect_contains(r, 14, 5));
        assert!(!rect_contains(r, 10, 8));
        assert!(!rect_contains(r, 9, 5));
    }

    #[test]
    fn rect_shrink_strips_n_cells_each_side() {
        let r = Rect::new(10, 5, 20, 10);
        let inner = rect_shrink(r, 1);
        assert_eq!(inner, Rect::new(11, 6, 18, 8));
    }

    #[test]
    fn rect_shrink_clamps_to_zero_when_too_small() {
        let r = Rect::new(0, 0, 2, 2);
        let inner = rect_shrink(r, 4);
        assert_eq!(inner.width, 0);
        assert_eq!(inner.height, 0);
    }

    #[test]
    fn palette_show_resets_state_to_visible_and_unfiltered() {
        let mut p = Palette::new();
        p.query = "stale".into();
        p.selected = 4;
        p.show();
        assert!(p.open);
        assert!(p.query.is_empty());
        assert_eq!(p.selected, 0);
        assert_eq!(p.filtered.len(), p.entries.len());
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
