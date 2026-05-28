//! # aonyx-tui
//!
//! Interactive terminal UI built on `ratatui` + `crossterm`.
//!
//! Planned components (Vague 1.5):
//! - **Composer** — sticky multi-line input with history.
//! - **Viewport** — scrollable message log with streaming token rendering.
//! - **Tool log** — collapsible per-tool-call panel.
//! - **Status bar** — provider, model, project, git branch, token usage.
//! - **OSC-52 clipboard** integration.
//! - **Slash command** auto-complete.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

// TODO(V1.5): scaffolded as a placeholder so `cargo check` sees the crate.
