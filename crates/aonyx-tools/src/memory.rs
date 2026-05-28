//! Memory tools exposed to the LLM: `memory_diary_append`, `memory_search`.
//!
//! These wrap the [`aonyx_core::MemoryStore`] trait so the agent can act on
//! its own palace from inside a turn.
//!
//! TODO(P1.5): wire these against a Palace handed in by the agent runner.
