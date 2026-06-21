//! # aonyx-api
//!
//! The Aonyx Agent **automation API**: a REST + WebSocket surface over the
//! same core that powers the CLI/TUI. It is a thin transport — every
//! endpoint bottoms out in the existing `aonyx-*` crates, no agent-loop
//! logic is reimplemented here.
//!
//! This crate is feature-driven by the binary: `aonyx-agent` depends on it
//! behind its `api` feature and serves [`build_router`] via
//! `aonyx serve api`. To avoid a dependency cycle, `aonyx-api` never
//! depends on `aonyx-agent`; the agent loop is injected through a trait
//! (added in a later phase), exactly like the channel adapters.
//!
//! ## Surface (grown phase by phase)
//! - **V4.1 (this scaffold)** — [`build_router`] with `/v1/health` (open)
//!   and `/v1/info` (bearer-authed), [`ApiState`], [`AuthConfig`],
//!   [`ServerInfo`], and the [`ApiError`] response type.
//! - V4.2 — sessions + blocking turns.
//! - V4.3 — WebSocket / SSE streaming.
//! - V4.4 — memory palace, tools, skills, config, OpenAPI.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

mod agent;
mod auth;
pub mod error;
mod memory;
mod meta;
mod openai;
mod routes;
mod server;
mod sessions;
mod state;
mod streaming;

pub use agent::{ApiAgent, ConfigInfo, SkillInfo, StreamFrame, ToolInfo};
pub use error::{ApiError, ApiResult};
pub use routes::build_router;
pub use server::serve;
pub use state::{ApiState, ApprovalHub, AuthConfig, ServerInfo};
