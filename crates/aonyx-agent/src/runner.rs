//! Main agent loop.
//!
//! Port reference: Aonyx RAG `rag_system/agent/runner.py` (lines 282-400).

use aonyx_core::Result;

/// Drives a session through one or more turns until the model emits no further tool calls.
#[derive(Default)]
pub struct AgentRunner {
    /// Maximum tool-call iterations per turn before bailing out.
    pub max_iterations: usize,
}

impl AgentRunner {
    /// Construct a runner with default limits.
    pub fn new() -> Self {
        Self { max_iterations: 10 }
    }

    /// Run a single turn. TODO(V1): wire LLM router, memory recall, skills, tools.
    pub async fn turn(&self, _user_msg: &str) -> Result<String> {
        Ok(String::new())
    }
}
