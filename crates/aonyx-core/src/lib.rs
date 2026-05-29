//! # aonyx-core
//!
//! Shared domain types, traits, and the canonical error for every other crate
//! in the Aonyx Agent workspace.
//!
//! This crate is **I/O-free**: it defines the vocabulary, not the behavior.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod error;
pub mod traits;
pub mod types;

pub use error::{AonyxError, Result};
pub use traits::{
    ChatChunk, ChatRequest, ChatStream, LlmProvider, MemoryStore, SkillSource, ToolHandler,
};
pub use types::{Attachment, Message, Role, SafetyClass, Session, ToolCall, ToolResult};
