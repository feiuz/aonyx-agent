//! # aonyx-agent
//!
//! The agent loop and its inner subsystems.
//!
//! ## Subsystems
//! - [`runner`] — the main `AgentRunner::run(session, msg)` loop.
//! - [`compaction`] — context-window pressure monitor + summarization triggers.
//! - [`classifier`] — routes user input to a prompt template (chitchat / task / recall / code / research).
//! - [`approval`] — gate around destructive tool calls.
//! - [`subagent`] — spawn isolated child agents with a whitelisted tool set (V2).
//!
//! ## Loop sketch
//! ```text
//! loop {
//!   inject(skills_active(query, project));
//!   inject(memory_recall(query, k=10));
//!   chunks = llm.chat_stream(messages, tools).await;
//!   for tool_call in chunks.tool_calls() {
//!       approval.check(tool_call)?;
//!       result = tools.invoke(tool_call).await?;
//!       messages.push(result);
//!   }
//!   if chunks.no_tool_call() { break; }
//! }
//! post_turn::maybe_diary_append();
//! post_turn::maybe_kg_upsert();
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod approval;
pub mod classifier;
pub mod compaction;
pub mod runner;
pub mod subagent;

pub use approval::ApprovalPolicy;
pub use runner::{AgentRunner, TurnResult};
