//! Memory tools exposed to the LLM: `memory_search`, `memory_kg_query`,
//! `memory_diary_append`.
//!
//! These wrap the [`aonyx_core::MemoryStore`] trait so the agent can act on its
//! own palace from inside a turn.
