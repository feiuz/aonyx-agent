//! Main agent loop.
//!
//! `AgentRunner::run(messages)` drives a conversation forward:
//! 1. Build a [`ChatRequest`] from the current message history and the
//!    schemas of every registered tool.
//! 2. Stream the response from the provider, accumulating text deltas and
//!    collecting tool calls.
//! 3. Append the assistant text as a [`Role::Assistant`] message.
//! 4. For each tool call, ask the [`ApprovalPolicy`], invoke the handler,
//!    and append the result as a [`Role::Tool`] message.
//! 5. If the turn produced **no** tool calls, return — the model is done.
//!    Otherwise loop, bounded by `max_iterations`.

use std::sync::Arc;

use aonyx_core::{
    AonyxError, ChatRequest, LlmProvider, Message, Result, Role, ToolCall, ToolHandler, ToolResult,
};
use aonyx_skills::{Skill, SkillEngine};
use aonyx_tools::ToolRegistry;
use futures::StreamExt;
use serde_json::{json, Value};

use crate::approval::ApprovalPolicy;

/// Outcome of a [`AgentRunner::run`] call.
#[derive(Debug, Clone)]
pub struct TurnResult {
    /// The full message log at the end of the run (input + assistant + tool messages).
    pub messages: Vec<Message>,
    /// Number of provider turns consumed.
    pub iterations: usize,
    /// `true` when the loop bailed out at `max_iterations`.
    pub max_iterations_hit: bool,
}

/// Drives a session forward, multi-turn, until the model emits no tool call
/// or the iteration cap is reached.
pub struct AgentRunner {
    provider: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    skills: Vec<Skill>,
    project: Option<String>,
    approval: ApprovalPolicy,
    model: String,
    max_iterations: usize,
}

impl AgentRunner {
    /// Construct a runner with the V1 default policy ([`ApprovalPolicy::DenyDestructive`]).
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            tools,
            skills: Vec::new(),
            project: None,
            approval: ApprovalPolicy::default(),
            model: model.into(),
            max_iterations: 10,
        }
    }

    /// Override the approval policy.
    pub fn with_approval(mut self, policy: ApprovalPolicy) -> Self {
        self.approval = policy;
        self
    }

    /// Override the per-turn iteration cap.
    pub fn with_max_iterations(mut self, n: usize) -> Self {
        self.max_iterations = n.max(1);
        self
    }

    /// Register a skill catalogue. Active skills are matched per turn against
    /// the latest user message + the (optional) project slug.
    pub fn with_skills(mut self, skills: Vec<Skill>) -> Self {
        self.skills = skills;
        self
    }

    /// Set the project slug used for project-pattern skill triggers.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    fn tools_schema(&self) -> Vec<Value> {
        let mut names: Vec<&str> = self.tools.names().collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|n| {
                let h = self.tools.get(n)?;
                Some(json!({
                    "name": n,
                    "description": "",
                    "input_schema": h.schema(),
                }))
            })
            .collect()
    }

    fn inject_active_skills(&self, messages: &mut Vec<Message>) {
        if self.skills.is_empty() {
            return;
        }
        let latest_user = messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let engine = SkillEngine::new(self.skills.clone());
        let active = engine.match_active(latest_user, self.project.as_deref());
        if active.is_empty() {
            return;
        }
        let block = active
            .iter()
            .map(|s| format!("# Skill: {}\n\n{}", s.name, s.body))
            .collect::<Vec<_>>()
            .join("\n\n");
        messages.insert(0, Message::new(Role::System, block));
    }

    /// Run the loop. `messages` is the full input transcript (system + user).
    pub async fn run(&self, mut messages: Vec<Message>) -> Result<TurnResult> {
        self.inject_active_skills(&mut messages);
        let tools = self.tools_schema();
        let mut iterations: usize = 0;

        for i in 0..self.max_iterations {
            iterations = i + 1;
            let req = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                temperature: None,
                max_tokens: None,
            };

            let (text, tool_calls) = self.consume_stream(req).await?;

            if !text.is_empty() {
                messages.push(Message::new(Role::Assistant, text));
            }

            if tool_calls.is_empty() {
                return Ok(TurnResult {
                    messages,
                    iterations,
                    max_iterations_hit: false,
                });
            }

            for call in tool_calls {
                let result = self.dispatch_tool(call).await;
                let payload = match result {
                    Ok(tr) => format_tool_result(&tr),
                    Err(e) => format!("[tool error] {e}"),
                };
                messages.push(Message::new(Role::Tool, payload));
            }
        }

        Ok(TurnResult {
            messages,
            iterations,
            max_iterations_hit: true,
        })
    }

    async fn consume_stream(&self, req: ChatRequest) -> Result<(String, Vec<ToolCall>)> {
        let mut stream = self.provider.chat_stream(req).await?;
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        while let Some(item) = stream.next().await {
            let chunk = item?;
            if !chunk.delta_text.is_empty() {
                text.push_str(&chunk.delta_text);
            }
            if let Some(tc) = chunk.tool_call {
                tool_calls.push(tc);
            }
            if chunk.finished {
                break;
            }
        }

        Ok((text, tool_calls))
    }

    async fn dispatch_tool(&self, call: ToolCall) -> Result<ToolResult> {
        let handler: Arc<dyn ToolHandler> = self
            .tools
            .get(&call.name)
            .ok_or_else(|| AonyxError::Tool(format!("unknown tool: {}", call.name)))?;
        let class = handler.classify();
        if !self.approval.allow(&call, class) {
            return Err(AonyxError::ApprovalRejected(format!(
                "{} ({:?})",
                call.name, class
            )));
        }
        handler.invoke(call).await
    }
}

fn format_tool_result(tr: &ToolResult) -> String {
    if let Some(err) = &tr.error {
        return format!("[tool error] {err}");
    }
    match serde_json::to_string_pretty(&tr.output) {
        Ok(s) => s,
        Err(_) => tr.output.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::{ChatChunk, ChatStream, Result as CoreResult};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Test double: each `chat_stream` call returns the next pre-canned chunk list.
    struct FakeProvider {
        queue: Mutex<Vec<Vec<ChatChunk>>>,
    }

    impl FakeProvider {
        fn new(responses: Vec<Vec<ChatChunk>>) -> Self {
            Self {
                queue: Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for FakeProvider {
        fn name(&self) -> &str {
            "fake"
        }

        async fn chat_stream(&self, _req: ChatRequest) -> CoreResult<ChatStream> {
            let mut q = self.queue.lock().expect("queue poisoned");
            let next = if q.is_empty() {
                Vec::new()
            } else {
                q.remove(0)
            };
            let stream = futures::stream::iter(next.into_iter().map(Ok));
            Ok(Box::pin(stream))
        }
    }

    fn text_chunk(s: &str) -> ChatChunk {
        ChatChunk {
            delta_text: s.to_string(),
            tool_call: None,
            finished: false,
        }
    }

    fn stop_chunk() -> ChatChunk {
        ChatChunk {
            delta_text: String::new(),
            tool_call: None,
            finished: true,
        }
    }

    fn tool_chunk(name: &str, args: Value) -> ChatChunk {
        ChatChunk {
            delta_text: String::new(),
            tool_call: Some(ToolCall {
                id: format!("call-{name}"),
                name: name.to_string(),
                args,
            }),
            finished: false,
        }
    }

    #[tokio::test]
    async fn terminates_when_no_tool_calls() {
        let provider = Arc::new(FakeProvider::new(vec![vec![
            text_chunk("Hello, "),
            text_chunk("world."),
            stop_chunk(),
        ]]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "any-model");
        let res = runner
            .run(vec![Message::new(Role::User, "hi")])
            .await
            .unwrap();
        assert_eq!(res.iterations, 1);
        assert!(!res.max_iterations_hit);
        assert_eq!(res.messages.len(), 2);
        assert_eq!(res.messages[1].role, Role::Assistant);
        assert_eq!(res.messages[1].content, "Hello, world.");
    }

    #[tokio::test]
    async fn loops_until_no_more_tool_calls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        tokio::fs::write(&path, "hello").await.unwrap();

        let provider = Arc::new(FakeProvider::new(vec![
            // Turn 1: ask for fs_read, no text.
            vec![
                tool_chunk("fs_read", json!({ "path": path.to_string_lossy() })),
                stop_chunk(),
            ],
            // Turn 2: produce final text, no tool call.
            vec![text_chunk("read it."), stop_chunk()],
        ]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "any-model");
        let res = runner
            .run(vec![Message::new(Role::User, "show me the file")])
            .await
            .unwrap();
        assert_eq!(res.iterations, 2);
        // User · Tool result · Assistant
        let roles: Vec<_> = res.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, vec![Role::User, Role::Tool, Role::Assistant]);
        assert!(res.messages[1].content.contains("hello"));
    }

    #[tokio::test]
    async fn respects_max_iterations() {
        // Provider always wants another tool call.
        let provider = Arc::new(FakeProvider::new(vec![
            vec![tool_chunk("git_status", json!({})), stop_chunk()],
            vec![tool_chunk("git_status", json!({})), stop_chunk()],
            vec![tool_chunk("git_status", json!({})), stop_chunk()],
        ]));
        let runner =
            AgentRunner::new(provider, ToolRegistry::default_set(), "m").with_max_iterations(2);
        let res = runner
            .run(vec![Message::new(Role::User, "loop forever")])
            .await
            .unwrap();
        assert_eq!(res.iterations, 2);
        assert!(res.max_iterations_hit);
    }

    #[tokio::test]
    async fn default_policy_blocks_destructive_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("forbidden.txt");
        let provider = Arc::new(FakeProvider::new(vec![vec![
            tool_chunk(
                "fs_write",
                json!({ "path": path.to_string_lossy(), "content": "nope" }),
            ),
            stop_chunk(),
        ]]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "m");
        let res = runner
            .run(vec![Message::new(Role::User, "write to disk")])
            .await
            .unwrap();
        let last = res.messages.last().unwrap();
        assert_eq!(last.role, Role::Tool);
        assert!(last.content.contains("approval rejected"));
        assert!(!path.exists(), "file must not have been written");
    }

    #[tokio::test]
    async fn auto_allow_lets_destructive_writes_through() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.txt");
        let provider = Arc::new(FakeProvider::new(vec![
            vec![
                tool_chunk(
                    "fs_write",
                    json!({ "path": path.to_string_lossy(), "content": "yes" }),
                ),
                stop_chunk(),
            ],
            vec![text_chunk("done"), stop_chunk()],
        ]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "m")
            .with_approval(ApprovalPolicy::AutoAllow);
        let res = runner
            .run(vec![Message::new(Role::User, "write to disk")])
            .await
            .unwrap();
        assert_eq!(res.iterations, 2);
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "yes");
    }
}
