//! Sub-agent dispatch (ADR-017).
//!
//! The architect (the main chat agent) carries the [`DispatchAgent`] tool. When
//! the model decides a task fits a specialised sub-agent, it calls
//! `dispatch_agent { agent, task }`; we spawn a fresh [`AgentRunner`] scoped to
//! that agent's tool whitelist + model, run the task to completion, and return
//! the sub-agent's final reply.
//!
//! Sub-agents share the project's memory palace — the memory tools in the
//! filtered registry carry the same `Palace` handle — so delegation never loses
//! context (the Aonyx memory-first twist). The sub-agent registry never contains
//! `dispatch_agent` (no recursion), and runs under `DenyDestructive` like its
//! parent (safe by default; write access arrives with the approval UX).

use std::path::Path;
use std::sync::Arc;

use aonyx_core::{
    AonyxError, LlmProvider, Message, Result, Role, SafetyClass, ToolCall, ToolHandler, ToolResult,
};
use aonyx_tools::ToolRegistry;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agents::{load_all, AgentDefinition};
use crate::approval::ApprovalPolicy;
use crate::runner::AgentRunner;

/// Run `task` through the sub-agent `def`, returning its final reply text.
///
/// `base` is the parent registry **without** `dispatch_agent`; the sub-agent
/// gets `base.subset(&def.tools)` (an empty whitelist inherits all of `base`).
pub async fn run_subagent(
    def: &AgentDefinition,
    provider: Arc<dyn LlmProvider>,
    base: &ToolRegistry,
    default_model: &str,
    task: &str,
) -> Result<String> {
    let tools = base.subset(&def.tools);
    let model = def
        .model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| default_model.to_string());
    let runner = AgentRunner::new(provider, tools, model)
        .with_approval(ApprovalPolicy::DenyDestructive)
        .with_max_iterations(def.max_iterations.unwrap_or(10));
    let messages = vec![
        Message::new(Role::System, def.body.clone()),
        Message::new(Role::User, task.to_string()),
    ];
    let result = runner.run(messages).await?;
    let reply = result
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant && !m.content.trim().is_empty())
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "(the sub-agent produced no text reply)".to_string());
    Ok(reply)
}

/// The `dispatch_agent` tool handed to the architect. Holds the base registry
/// (without `dispatch_agent`), the parent provider + default model, and the
/// catalogue of available sub-agents.
pub struct DispatchAgent {
    base: ToolRegistry,
    provider: Arc<dyn LlmProvider>,
    default_model: String,
    agents: Vec<AgentDefinition>,
}

impl DispatchAgent {
    /// Build from the parent's registry (cloned **before** `dispatch_agent` is
    /// registered), provider, default model, and agent catalogue.
    pub fn new(
        base: ToolRegistry,
        provider: Arc<dyn LlmProvider>,
        default_model: impl Into<String>,
        agents: Vec<AgentDefinition>,
    ) -> Self {
        Self {
            base,
            provider,
            default_model: default_model.into(),
            agents,
        }
    }
}

#[async_trait]
impl ToolHandler for DispatchAgent {
    fn name(&self) -> &str {
        "dispatch_agent"
    }

    fn classify(&self) -> SafetyClass {
        // Delegation itself is safe; the sub-agent's own tool calls are gated by
        // its (DenyDestructive) policy and whitelist.
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        let roster: String = self
            .agents
            .iter()
            .map(|a| format!("- {}: {}", a.id, a.description))
            .collect::<Vec<_>>()
            .join("\n");
        let ids: Vec<&str> = self.agents.iter().map(|a| a.id.as_str()).collect();
        json!({
            "description": format!(
                "Delegate a self-contained sub-task to a specialised sub-agent and return its \
                 result. Use it when a task clearly fits one of these agents:\n{roster}"
            ),
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "enum": ids,
                    "description": "Which sub-agent to delegate to."
                },
                "task": {
                    "type": "string",
                    "description": "A complete, standalone instruction for the sub-agent — it does NOT see this conversation."
                }
            },
            "required": ["agent", "task"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let agent_id = call
            .args
            .get("agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AonyxError::Tool("dispatch_agent: missing 'agent'".into()))?;
        let task = call
            .args
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AonyxError::Tool("dispatch_agent: missing 'task'".into()))?;
        let def = self.agents.iter().find(|a| a.id == agent_id).ok_or_else(|| {
            let have = self
                .agents
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            AonyxError::Tool(format!(
                "dispatch_agent: unknown agent '{agent_id}' (have: {have})"
            ))
        })?;
        let reply = run_subagent(
            def,
            Arc::clone(&self.provider),
            &self.base,
            &self.default_model,
            task,
        )
        .await?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "agent": def.id, "reply": reply }),
            error: None,
        })
    }
}

/// Register `dispatch_agent` into `registry`, sourcing sub-agents from the
/// built-in presets overlaid with `agents_dir` (`~/.aonyx/agents/`). No-op when
/// there are no agents. Call this **last** — after every other tool is
/// registered — so sub-agents inherit the full toolset (minus `dispatch_agent`).
pub fn register_dispatch_agent(
    registry: &mut ToolRegistry,
    provider: Arc<dyn LlmProvider>,
    default_model: impl Into<String>,
    agents_dir: impl AsRef<Path>,
) {
    let agents = load_all(agents_dir);
    if agents.is_empty() {
        return;
    }
    let base = registry.clone();
    let dispatch = DispatchAgent::new(base, provider, default_model, agents);
    registry.register(Arc::new(dispatch));
}
