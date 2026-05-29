//! # aonyx-tools
//!
//! The built-in tool catalogue. Every tool implements
//! [`ToolHandler`](aonyx_core::ToolHandler) and lives in its own module.
//!
//! ## V1 tools
//! - [`fs`] — `fs_read`, `fs_write`, `fs_edit`, `fs_glob`, `fs_grep`
//! - [`bash`] — sandboxed shell invocation with timeout
//! - [`git`] — `git_status`, `git_diff`, `git_log`, `git_show`
//! - [`exec`] — generic process execution
//! - [`web`] — `web_fetch`, `web_search` (Brave / Tavily)
//! - [`memory`] — `memory_search`, `memory_kg_query`, `memory_diary_append`
//!
//! ## Registry
//! [`ToolRegistry::default_set`] returns a registry pre-populated with every V1 tool.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod bash;
pub mod exec;
pub mod fs;
pub mod git;
pub mod memory;
pub mod registry;
pub mod undo;
pub mod web;

pub use registry::ToolRegistry;
