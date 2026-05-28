//! # aonyx-skills
//!
//! Skill subsystem: parse [`SKILL.md`](https://agentskills.io) files, match them
//! against the current context, inject their system prompts, and (V1.2+)
//! auto-generate new skills from recurring task shapes.
//!
//! ## SKILL.md format (agentskills.io-compatible)
//!
//! ```markdown
//! ---
//! id: code-review
//! name: Code Review
//! enabled: true
//! tools: [fs_read, fs_grep, git_diff]
//! trigger:
//!   keywords: ["review", "lgtm", "look at this PR"]
//!   query_matches: ["^review the (PR|diff)"]
//!   project_matches: "^aonyx-.*"
//!   manual: false
//!   always_on: false
//! ---
//!
//! You are a meticulous code reviewer. Focus on correctness, then clarity, then
//! style. Cite line numbers when you raise an issue.
//! ```
//!
//! ## V1 built-in skills (ported from Aonyx RAG)
//! - `code-review`
//! - `data-analyst`
//! - `doc-writer`
//! - `incident-response`

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod engine;
pub mod loader;
pub mod schema;

pub use schema::{Skill, Trigger};
