//! Main agent loop.
//!
//! Two entry points:
//!
//! - [`AgentRunner::run`] — drives a conversation forward and returns the
//!   full transcript at the end. Convenient for tests and batch callers.
//! - [`AgentRunner::run_streaming`] — same loop, but emits [`TurnEvent`]s on
//!   a channel as they happen (delta text, tool start / end, iteration
//!   boundaries, terminal `Done`). The interactive CLI uses this to render
//!   tokens as they arrive and annotate tool activity inline.
//!
//! Inner steps:
//! 1. Build a [`ChatRequest`] from the current message history and the
//!    schemas of every registered tool.
//! 2. Stream the response from the provider, accumulating text deltas and
//!    collecting tool calls. Each delta becomes a
//!    [`TurnEvent::AssistantDelta`].
//! 3. Append the assistant text as a [`Role::Assistant`] message.
//! 4. For each tool call, emit [`TurnEvent::ToolStart`], ask the
//!    [`ApprovalPolicy`], invoke the handler, emit [`TurnEvent::ToolEnd`] or
//!    [`TurnEvent::ToolRejected`], and append the result as a [`Role::Tool`]
//!    message.
//! 5. If the turn produced **no** tool calls, emit [`TurnEvent::Done`] and
//!    return. Otherwise loop, bounded by `max_iterations`.

use std::sync::Arc;

use aonyx_core::{
    AonyxError, ChatRequest, LlmProvider, Message, Result, Role, SafetyClass, ToolCall,
    ToolHandler, ToolResult,
};
use aonyx_skills::{Skill, SkillEngine};
use aonyx_tools::ToolRegistry;
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::approval::ApprovalPolicy;

/// Streamed observation of what the runner is doing right now.
#[derive(Debug, Clone)]
pub enum TurnEvent {
    /// Iteration index (1-based) about to start.
    IterationStart(usize),
    /// Incremental assistant text — render as soon as it arrives.
    AssistantDelta(String),
    /// The assistant emitted its final text for this iteration (no further
    /// streaming until the next iteration or tool call).
    AssistantMessageEnd,
    /// The model requested a tool. The approval gate has not run yet.
    ToolStart {
        /// Tool name (matches `ToolHandler::name`).
        name: String,
        /// JSON arguments the model is sending.
        args: Value,
        /// Safety class of the tool.
        class: SafetyClass,
    },
    /// A tool finished executing.
    ToolEnd {
        /// Tool name.
        name: String,
        /// `true` when the tool succeeded.
        ok: bool,
        /// Short one-line summary of the result (truncated).
        summary: String,
    },
    /// A tool call was rejected by the approval policy and never executed.
    ToolRejected {
        /// Tool name.
        name: String,
        /// Safety class that caused the rejection.
        class: SafetyClass,
    },
    /// A destructive call is paused awaiting the user's approval (interactive
    /// policy). The client prompts and resolves the decision keyed by `id`.
    ApprovalRequest {
        /// The tool call id — echo it back to resolve the decision.
        id: String,
        /// Tool name.
        name: String,
        /// JSON arguments the model wants to run with.
        args: Value,
        /// Safety class (destructive in V1).
        class: SafetyClass,
    },
    /// The loop finished — model emitted no tool call, or the iteration cap
    /// was hit.
    Done {
        /// Iterations consumed.
        iterations: usize,
        /// `true` when the loop bailed out at `max_iterations`.
        max_iterations_hit: bool,
    },
}

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
///
/// `Clone` is intentional: `Arc<AgentRunner>` is awkward because every field
/// is already cheaply cloneable, and the TUI spawns runner work onto its own
/// task which needs an owned value.
#[derive(Clone)]
pub struct AgentRunner {
    /// Active provider — shared behind an `Arc<Mutex<_>>` so the TUI
    /// `/provider` command (Phase LL) can swap the whole backend live.
    provider: Arc<std::sync::Mutex<Arc<dyn LlmProvider>>>,
    tools: ToolRegistry,
    skills: Vec<Skill>,
    /// Skill ids the user has switched off for this session — shared
    /// behind an `Arc<Mutex<_>>` so the TUI `/skills` panel (Phase X)
    /// can flip them live and the next turn picks up the change.
    disabled_skills: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Pretty-printed (redacted) JSON of the most recent request sent
    /// to the provider — surfaced by the TUI `/inspect` panel
    /// (Phase Y). `None` until the first turn fires.
    last_request: Arc<std::sync::Mutex<Option<String>>>,
    project: Option<String>,
    approval: ApprovalPolicy,
    /// Active model id — shared behind an `Arc<Mutex<_>>` so the TUI
    /// `/model` command (Phase EE) can swap it live and the next turn
    /// (and `summarize`) picks up the change.
    model: Arc<std::sync::Mutex<String>>,
    max_iterations: usize,
    /// Max `dispatch_agent` calls the architect may make in a single turn (MA4).
    /// Bounds sub-agent fan-out; recursion depth is already 1 by construction
    /// (sub-agents carry no `dispatch_agent`).
    max_dispatches: usize,
    /// Pre-fetch RAG context before each turn (retrieve-then-generate).
    auto_retrieve: bool,
    /// Chunks injected when [`Self::with_auto_retrieve`] is enabled.
    auto_retrieve_top_k: usize,
    /// Skip auto-retrieve for user messages shorter than this (chars).
    auto_retrieve_min_len: usize,
}

impl AgentRunner {
    /// Construct a runner with the V1 default policy ([`ApprovalPolicy::DenyDestructive`]).
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider: Arc::new(std::sync::Mutex::new(provider)),
            tools,
            skills: Vec::new(),
            disabled_skills: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            last_request: Arc::new(std::sync::Mutex::new(None)),
            project: None,
            approval: ApprovalPolicy::default(),
            model: Arc::new(std::sync::Mutex::new(model.into())),
            max_iterations: 10,
            max_dispatches: 8,
            auto_retrieve: false,
            auto_retrieve_top_k: 5,
            auto_retrieve_min_len: 12,
        }
    }

    /// Snapshot the active model id.
    fn current_model(&self) -> String {
        self.model.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// Share the live model handle so the TUI `/model` command can swap
    /// the active model mid-session (Phase EE).
    pub fn model_handle(&self) -> Arc<std::sync::Mutex<String>> {
        Arc::clone(&self.model)
    }

    /// Share the live provider handle so the TUI `/provider` command
    /// can swap the whole backend mid-session (Phase LL).
    pub fn provider_handle(&self) -> Arc<std::sync::Mutex<Arc<dyn LlmProvider>>> {
        Arc::clone(&self.provider)
    }

    /// Snapshot the active provider.
    fn current_provider(&self) -> Arc<dyn LlmProvider> {
        self.provider
            .lock()
            .map(|p| Arc::clone(&p))
            .unwrap_or_else(|e| Arc::clone(&e.into_inner()))
    }

    /// Share a live skill-toggle set with the caller. Skill ids present
    /// in the set are skipped during per-turn matching, letting the TUI
    /// enable / disable skills mid-session (Phase X).
    pub fn skill_toggle_handle(&self) -> Arc<std::sync::Mutex<std::collections::HashSet<String>>> {
        Arc::clone(&self.disabled_skills)
    }

    /// Share a handle to the most-recent-request snapshot. The TUI
    /// `/inspect` panel (Phase Y) reads the pretty-printed JSON written
    /// here on every turn.
    pub fn last_request_handle(&self) -> Arc<std::sync::Mutex<Option<String>>> {
        Arc::clone(&self.last_request)
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

    /// Override the per-turn `dispatch_agent` budget (MA4). `0` disables
    /// delegation entirely; the default is 8.
    pub fn with_max_dispatches(mut self, n: usize) -> Self {
        self.max_dispatches = n;
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

    /// Enable retrieve-then-generate. Before each turn, pre-fetch RAG context
    /// for the latest user message via the `rag_search` MCP tool and inject it
    /// as a system block — helps weaker models that don't reliably call the
    /// tool themselves on interrogative messages. The agent stays free to call
    /// `read_document` / `find_related` to dig deeper; this only pre-loads.
    /// No-op when no `…__rag_search` tool is registered, on slash-commands, or
    /// on messages shorter than `min_len` chars. `top_k` is clamped to 1..=10.
    pub fn with_auto_retrieve(mut self, enabled: bool, top_k: usize, min_len: usize) -> Self {
        self.auto_retrieve = enabled;
        self.auto_retrieve_top_k = top_k.clamp(1, 10);
        self.auto_retrieve_min_len = min_len;
        self
    }

    fn tools_schema(&self) -> Vec<Value> {
        let mut names: Vec<&str> = self.tools.names().collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|n| {
                let h = self.tools.get(n)?;
                let schema = h.schema();
                let description = schema
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(json!({
                    "name": n,
                    "description": description,
                    "input_schema": schema,
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

        // Phase X — drop skills the user toggled off before matching.
        let disabled = self
            .disabled_skills
            .lock()
            .map(|d| d.clone())
            .unwrap_or_default();
        let live_skills: Vec<Skill> = self
            .skills
            .iter()
            .filter(|s| !disabled.contains(&s.id))
            .cloned()
            .collect();
        if live_skills.is_empty() {
            return;
        }

        let engine = SkillEngine::new(live_skills);
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

    /// Retrieve-then-generate: pre-fetch RAG context for the latest user
    /// message and insert it as a system block right before that message.
    /// Best-effort — any failure (no tool, tool error, empty result) leaves
    /// `messages` untouched so the turn proceeds normally.
    async fn inject_auto_retrieve(&self, messages: &mut Vec<Message>) {
        if !self.auto_retrieve {
            return;
        }
        let Some(user_idx) = messages.iter().rposition(|m| m.role == Role::User) else {
            return;
        };
        let query = messages[user_idx].content.trim().to_string();
        // Skip slash-commands and very short messages ("ok", "merci", …).
        if query.starts_with('/') || query.chars().count() < self.auto_retrieve_min_len {
            return;
        }
        // MCP tools are named "<server>__<tool>" — match rag_search whatever
        // the configured server is called.
        let Some(tool_name) = self
            .tools
            .names()
            .find(|n| *n == "rag_search" || n.ends_with("__rag_search"))
            .map(|n| n.to_string())
        else {
            tracing::debug!("auto_retrieve: no rag_search tool registered; skipping");
            return;
        };
        let Some(handler) = self.tools.get(&tool_name) else {
            return;
        };
        let call = ToolCall {
            id: "auto-retrieve".to_string(),
            name: tool_name,
            args: json!({ "query": query, "top_k": self.auto_retrieve_top_k }),
        };
        let output = match handler.invoke(call).await {
            Ok(tr) if tr.error.is_none() => tr.output,
            Ok(tr) => {
                tracing::debug!(
                    "auto_retrieve: rag_search returned an error: {:?}",
                    tr.error
                );
                return;
            }
            Err(e) => {
                tracing::debug!("auto_retrieve: rag_search invoke failed: {e}");
                return;
            }
        };
        let Some(body) = format_retrieved_context(&output, self.auto_retrieve_top_k) else {
            return;
        };
        let block = format!(
            "[Contexte RAG pré-chargé pour la question — cite la source (projet / fichier) \
             si tu l'utilises ; tu peux approfondir avec read_document / find_related]\n\n{body}"
        );
        messages.insert(user_idx, Message::new(Role::System, block));
    }

    /// Run the loop and return the full transcript.
    ///
    /// Equivalent to [`AgentRunner::run_streaming`] with a discarded event
    /// channel — convenient for tests and batch callers that don't need
    /// progressive UI updates.
    pub async fn run(&self, messages: Vec<Message>) -> Result<TurnResult> {
        // Use a generous buffer so the synchronous sends inside the loop never
        // block on a missing receiver. The receiver here drains and ignores
        // every event.
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(256);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let result = self.run_streaming(messages, tx).await;
        drain.await.ok();
        result
    }

    /// Run the loop, emitting [`TurnEvent`]s on `events` as they happen.
    ///
    /// `events` is dropped when the function returns, which signals the
    /// caller that the run is complete (a receive on the matching receiver
    /// will then yield `None`).
    pub async fn run_streaming(
        &self,
        mut messages: Vec<Message>,
        events: mpsc::Sender<TurnEvent>,
    ) -> Result<TurnResult> {
        self.inject_active_skills(&mut messages);
        self.inject_auto_retrieve(&mut messages).await;
        let tools = self.tools_schema();
        let mut iterations: usize = 0;
        let mut dispatches: usize = 0;

        for i in 0..self.max_iterations {
            iterations = i + 1;
            let _ = events.send(TurnEvent::IterationStart(iterations)).await;

            let req = ChatRequest {
                model: self.current_model(),
                messages: messages.clone(),
                tools: tools.clone(),
                temperature: None,
                max_tokens: None,
            };

            // Phase Y — capture a redacted snapshot for `/inspect`
            // before the request leaves. Best-effort: a serialization
            // hiccup never blocks the turn.
            if let Ok(mut slot) = self.last_request.lock() {
                *slot = Some(redact_request_json(&req));
            }

            let (text, tool_calls) = self.consume_stream(req, &events).await?;

            if tool_calls.is_empty() {
                if !text.is_empty() {
                    messages.push(Message::new(Role::Assistant, text));
                }
                let _ = events.send(TurnEvent::AssistantMessageEnd).await;
                let _ = events
                    .send(TurnEvent::Done {
                        iterations,
                        max_iterations_hit: false,
                    })
                    .await;
                return Ok(TurnResult {
                    messages,
                    iterations,
                    max_iterations_hit: false,
                });
            }

            // The model requested tools — record the assistant turn carrying
            // both its text and the tool calls, so the next iteration replays
            // the request/response pair correctly to the provider.
            messages.push(Message::assistant_tool_calls(text, tool_calls.clone()));
            let _ = events.send(TurnEvent::AssistantMessageEnd).await;

            for call in tool_calls {
                // MA4 — bound sub-agent fan-out per turn. Depth is already 1
                // (sub-agents carry no `dispatch_agent`); this caps breadth so
                // the architect can't spawn an unbounded number of sub-agents.
                if call.name == "dispatch_agent" {
                    dispatches += 1;
                    if dispatches > self.max_dispatches {
                        messages.push(Message::tool_result(
                            call.id,
                            format!(
                                "[delegation budget exhausted] You have reached the \
                                 delegation limit ({} per turn). Do not call \
                                 dispatch_agent again — complete the task yourself or \
                                 summarise the sub-agents' results so far.",
                                self.max_dispatches
                            ),
                        ));
                        continue;
                    }
                }
                let class = self
                    .tools
                    .get(&call.name)
                    .map(|h| h.classify())
                    .unwrap_or(SafetyClass::Safe);

                // Approval gate. Interactive policies emit an ApprovalRequest and
                // pause here for the user's decision; the others resolve
                // synchronously (deny-destructive / auto-allow / custom).
                if !self.approve_call(&call, class, &events).await {
                    let _ = events
                        .send(TurnEvent::ToolRejected {
                            name: call.name.clone(),
                            class,
                        })
                        .await;
                    messages.push(Message::tool_result(
                        call.id,
                        format!("[approval rejected] {} ({:?})", call.name, class),
                    ));
                    continue;
                }

                let _ = events
                    .send(TurnEvent::ToolStart {
                        name: call.name.clone(),
                        args: call.args.clone(),
                        class,
                    })
                    .await;

                let outcome = self.dispatch_tool(call.clone()).await;
                let payload = match &outcome {
                    Ok(tr) => {
                        let _ = events
                            .send(TurnEvent::ToolEnd {
                                name: call.name.clone(),
                                ok: true,
                                summary: short_summary(&tr.output),
                            })
                            .await;
                        format_tool_result(tr)
                    }
                    Err(AonyxError::ApprovalRejected(_)) => {
                        let _ = events
                            .send(TurnEvent::ToolRejected {
                                name: call.name.clone(),
                                class,
                            })
                            .await;
                        format!("[approval rejected] {} ({:?})", call.name, class)
                    }
                    Err(e) => {
                        let msg = format!("{e}");
                        let _ = events
                            .send(TurnEvent::ToolEnd {
                                name: call.name.clone(),
                                ok: false,
                                summary: msg.clone(),
                            })
                            .await;
                        format!("[tool error] {msg}")
                    }
                };
                messages.push(Message::tool_result(call.id, payload));
            }
        }

        let _ = events
            .send(TurnEvent::Done {
                iterations,
                max_iterations_hit: true,
            })
            .await;
        Ok(TurnResult {
            messages,
            iterations,
            max_iterations_hit: true,
        })
    }

    /// Summarize a slice of conversation into a single compact paragraph
    /// (Phase BB). One-shot, tool-free, non-streaming — used by the TUI
    /// auto-compaction to fold old turns into a system note.
    pub async fn summarize(&self, history: &[Message]) -> Result<String> {
        let transcript = history
            .iter()
            .map(|m| {
                let who = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                format!("{who}: {}", m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = "You are compacting a conversation to save context. Summarize the \
            exchange below concisely, preserving key facts, decisions, file paths, \
            identifiers, and any open questions or TODOs. Omit pleasantries. Output \
            only the summary prose — no preamble.";
        let req = ChatRequest {
            model: self.current_model(),
            messages: vec![
                Message::new(Role::System, prompt),
                Message::new(Role::User, transcript),
            ],
            tools: Vec::new(),
            temperature: Some(0.0),
            max_tokens: Some(1024),
        };

        let provider = self.current_provider();
        let mut stream = provider.chat_stream(req).await?;
        let mut text = String::new();
        while let Some(item) = stream.next().await {
            let chunk = item?;
            text.push_str(&chunk.delta_text);
            if chunk.finished {
                break;
            }
        }
        Ok(text.trim().to_string())
    }

    async fn consume_stream(
        &self,
        req: ChatRequest,
        events: &mpsc::Sender<TurnEvent>,
    ) -> Result<(String, Vec<ToolCall>)> {
        let provider = self.current_provider();
        let mut stream = provider.chat_stream(req).await?;
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        while let Some(item) = stream.next().await {
            let chunk = item?;
            if !chunk.delta_text.is_empty() {
                let _ = events
                    .send(TurnEvent::AssistantDelta(chunk.delta_text.clone()))
                    .await;
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
        // The approval gate runs in the turn loop (it needs the event channel to
        // drive an interactive approver); by here the call is already cleared.
        handler.invoke(call).await
    }

    /// Run the approval gate for `call`. An [`ApprovalPolicy::Interactive`]
    /// policy emits an [`TurnEvent::ApprovalRequest`] (keyed by the tool call
    /// id) and awaits the user's decision; every other policy resolves
    /// synchronously. Only destructive calls are ever paused.
    async fn approve_call(
        &self,
        call: &ToolCall,
        class: SafetyClass,
        events: &mpsc::Sender<TurnEvent>,
    ) -> bool {
        if let ApprovalPolicy::Interactive(approver) = &self.approval {
            if class == SafetyClass::Destructive {
                let _ = events
                    .send(TurnEvent::ApprovalRequest {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        args: call.args.clone(),
                        class,
                    })
                    .await;
                return approver.approve(&call.id, call, class).await;
            }
            return true;
        }
        self.approval.allow(call, class).await
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

/// Serialize a [`ChatRequest`] to pretty JSON for the `/inspect` panel
/// (Phase Y), eliding base64 image payloads so the snapshot stays
/// readable (a single PNG can be hundreds of KB of base64).
fn redact_request_json(req: &ChatRequest) -> String {
    let mut value = match serde_json::to_value(req) {
        Ok(v) => v,
        Err(e) => return format!("(could not serialize request: {e})"),
    };
    if let Some(messages) = value.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            if let Some(atts) = msg.get_mut("attachments").and_then(|a| a.as_array_mut()) {
                for att in atts.iter_mut() {
                    if let Some(data) = att.get_mut("data") {
                        if let Some(s) = data.as_str() {
                            *data = Value::String(format!("<{} bytes base64 elided>", s.len()));
                        }
                    }
                }
            }
        }
    }
    serde_json::to_string_pretty(&value).unwrap_or_else(|e| format!("(pretty-print failed: {e})"))
}

fn short_summary(value: &Value) -> String {
    let raw = match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    let trimmed = raw.replace('\n', " ");
    if trimmed.chars().count() > 120 {
        let cut: String = trimmed.chars().take(120).collect();
        format!("{cut}…")
    } else {
        trimmed
    }
}

/// Format a `rag_search` tool result into a compact context block for
/// [`AgentRunner::inject_auto_retrieve`]. Handles both a structured
/// `{ "results": [{project, source, content}, …] }` payload and the MCP
/// text-content case (where the server's JSON arrives as a single string —
/// see `aonyx_mcp`'s `extract_call_result`). Returns `None` when there's
/// nothing usable to inject.
fn format_retrieved_context(output: &Value, top_k: usize) -> Option<String> {
    if let Some(results) = output.get("results").and_then(|r| r.as_array()) {
        return format_results_array(results, top_k);
    }
    if let Some(s) = output.as_str() {
        // The MCP server's response often arrives as a JSON string.
        if let Ok(parsed) = serde_json::from_str::<Value>(s) {
            if let Some(results) = parsed.get("results").and_then(|r| r.as_array()) {
                if let Some(block) = format_results_array(results, top_k) {
                    return Some(block);
                }
            }
        }
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(cap(trimmed, 6000));
    }
    None
}

/// Render a `results` array (`{project, source, content}` objects) as a
/// bulleted, source-attributed context block, capped per chunk.
fn format_results_array(results: &[Value], top_k: usize) -> Option<String> {
    let mut blocks = Vec::new();
    for r in results.iter().take(top_k) {
        let content = r
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if content.is_empty() {
            continue;
        }
        let project = r.get("project").and_then(|v| v.as_str()).unwrap_or("?");
        let source = r.get("source").and_then(|v| v.as_str()).unwrap_or("?");
        blocks.push(format!(
            "- (projet {project} / {source})\n{}",
            cap(content, 1200)
        ));
    }
    if blocks.is_empty() {
        None
    } else {
        Some(blocks.join("\n\n"))
    }
}

/// Truncate `s` to at most `max_chars` characters, appending an ellipsis
/// when it was cut.
fn cap(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars).collect();
        format!("{head}…")
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

    fn drain<T>(rx: &mut mpsc::Receiver<T>) -> Vec<T> {
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn always_on_skill(id: &str, body: &str) -> Skill {
        let mut s = Skill {
            id: id.to_string(),
            name: id.to_string(),
            enabled: true,
            tools: Vec::new(),
            trigger: Default::default(),
            body: body.to_string(),
        };
        s.trigger.always_on = true;
        s
    }

    #[tokio::test]
    async fn summarize_collects_streamed_text() {
        let provider = Arc::new(FakeProvider::new(vec![vec![
            text_chunk("Summary: "),
            text_chunk("user asked about X."),
            stop_chunk(),
        ]]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "any-model");
        let history = vec![
            Message::new(Role::User, "tell me about X"),
            Message::new(Role::Assistant, "X is a thing"),
        ];
        let summary = runner.summarize(&history).await.unwrap();
        assert_eq!(summary, "Summary: user asked about X.");
    }

    #[test]
    fn redact_request_json_elides_image_payloads() {
        use aonyx_core::Attachment;
        let req = ChatRequest {
            model: "claude-x".to_string(),
            messages: vec![Message::with_attachments(
                Role::User,
                "look",
                vec![Attachment::Image {
                    media_type: "image/png".into(),
                    data: "A".repeat(5000),
                }],
            )],
            tools: vec![],
            temperature: None,
            max_tokens: None,
        };
        let json = redact_request_json(&req);
        assert!(json.contains("claude-x"));
        assert!(json.contains("image/png"));
        // The 5000-char blob must be gone, replaced by the elision tag.
        assert!(!json.contains(&"A".repeat(5000)));
        assert!(json.contains("base64 elided"));
    }

    #[test]
    fn redact_request_json_passes_text_only_requests_through() {
        let req = ChatRequest {
            model: "m".to_string(),
            messages: vec![Message::new(Role::User, "plain text")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
        };
        let json = redact_request_json(&req);
        assert!(json.contains("plain text"));
    }

    #[test]
    fn inject_active_skills_adds_an_always_on_skill() {
        let runner = AgentRunner::new(
            Arc::new(FakeProvider::new(vec![])),
            ToolRegistry::default_set(),
            "any-model",
        )
        .with_skills(vec![always_on_skill("greeter", "ALWAYS GREET")]);
        let mut messages = vec![Message::new(Role::User, "hi")];
        runner.inject_active_skills(&mut messages);
        assert_eq!(messages[0].role, Role::System);
        assert!(messages[0].content.contains("ALWAYS GREET"));
    }

    #[test]
    fn disabled_skill_is_not_injected() {
        let runner = AgentRunner::new(
            Arc::new(FakeProvider::new(vec![])),
            ToolRegistry::default_set(),
            "any-model",
        )
        .with_skills(vec![always_on_skill("greeter", "ALWAYS GREET")]);
        // Toggle the skill off through the shared handle.
        runner
            .skill_toggle_handle()
            .lock()
            .unwrap()
            .insert("greeter".to_string());
        let mut messages = vec![Message::new(Role::User, "hi")];
        runner.inject_active_skills(&mut messages);
        // No system block injected; the lone user message stands.
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
    }

    #[test]
    fn re_enabling_a_skill_restores_injection() {
        let runner = AgentRunner::new(
            Arc::new(FakeProvider::new(vec![])),
            ToolRegistry::default_set(),
            "any-model",
        )
        .with_skills(vec![always_on_skill("greeter", "ALWAYS GREET")]);
        let handle = runner.skill_toggle_handle();
        handle.lock().unwrap().insert("greeter".to_string());
        handle.lock().unwrap().remove("greeter");
        let mut messages = vec![Message::new(Role::User, "hi")];
        runner.inject_active_skills(&mut messages);
        assert_eq!(messages[0].role, Role::System);
        assert!(messages[0].content.contains("ALWAYS GREET"));
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
        // User · Assistant(tool_use) · Tool result · Assistant(final).
        // The assistant turn that requested the tool is now recorded with
        // its tool_calls, and the result links back via tool_call_id.
        let roles: Vec<_> = res.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![Role::User, Role::Assistant, Role::Tool, Role::Assistant]
        );
        assert_eq!(res.messages[1].tool_calls.len(), 1);
        assert_eq!(res.messages[1].tool_calls[0].name, "fs_read");
        assert!(res.messages[2].tool_call_id.is_some());
        assert!(res.messages[2].content.contains("hello"));
        assert_eq!(res.messages[3].content, "read it.");
    }

    #[tokio::test]
    async fn respects_max_iterations() {
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
    async fn respects_max_dispatches() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // A stand-in `dispatch_agent` that counts how often it actually runs.
        struct CountingDispatch(Arc<AtomicUsize>);
        #[async_trait::async_trait]
        impl ToolHandler for CountingDispatch {
            fn name(&self) -> &str {
                "dispatch_agent"
            }
            fn schema(&self) -> Value {
                json!({ "name": "dispatch_agent", "description": "stub" })
            }
            fn classify(&self) -> SafetyClass {
                SafetyClass::Safe
            }
            async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(ToolResult {
                    call_id: call.id,
                    output: json!("ok"),
                    error: None,
                })
            }
        }

        let runs = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::default_set();
        registry.register(Arc::new(CountingDispatch(Arc::clone(&runs))));

        // One turn that delegates three times, then a final text turn.
        let provider = Arc::new(FakeProvider::new(vec![
            vec![
                tool_chunk("dispatch_agent", json!({ "agent": "coder", "task": "a" })),
                tool_chunk("dispatch_agent", json!({ "agent": "coder", "task": "b" })),
                tool_chunk("dispatch_agent", json!({ "agent": "coder", "task": "c" })),
                stop_chunk(),
            ],
            vec![text_chunk("done."), stop_chunk()],
        ]));
        let runner = AgentRunner::new(provider, registry, "m")
            .with_approval(ApprovalPolicy::AutoAllow)
            .with_max_dispatches(2);
        let res = runner
            .run(vec![Message::new(Role::User, "delegate a lot")])
            .await
            .unwrap();

        // Only two delegations executed; the third was budget-capped.
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert!(res
            .messages
            .iter()
            .any(|m| m.role == Role::Tool && m.content.contains("delegation budget exhausted")));
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

    #[tokio::test]
    async fn run_streaming_emits_delta_events_in_order() {
        let provider = Arc::new(FakeProvider::new(vec![vec![
            text_chunk("Hello"),
            text_chunk(", "),
            text_chunk("world"),
            stop_chunk(),
        ]]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "m");
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
        runner
            .run_streaming(vec![Message::new(Role::User, "hi")], tx)
            .await
            .unwrap();

        let events = drain(&mut rx);
        let deltas: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                TurnEvent::AssistantDelta(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec!["Hello", ", ", "world"]);

        let has_done = events.iter().any(|e| {
            matches!(
                e,
                TurnEvent::Done {
                    max_iterations_hit: false,
                    ..
                }
            )
        });
        assert!(has_done);
    }

    #[tokio::test]
    async fn run_streaming_announces_tool_start_and_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.txt");
        tokio::fs::write(&path, "ok").await.unwrap();

        let provider = Arc::new(FakeProvider::new(vec![
            vec![
                tool_chunk("fs_read", json!({ "path": path.to_string_lossy() })),
                stop_chunk(),
            ],
            vec![text_chunk("done"), stop_chunk()],
        ]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "m");
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
        runner
            .run_streaming(vec![Message::new(Role::User, "read it")], tx)
            .await
            .unwrap();

        let events = drain(&mut rx);
        let start_seen = events
            .iter()
            .any(|e| matches!(e, TurnEvent::ToolStart { name, .. } if name == "fs_read"));
        let end_seen = events
            .iter()
            .any(|e| matches!(e, TurnEvent::ToolEnd { name, ok: true, .. } if name == "fs_read"));
        assert!(start_seen, "expected ToolStart for fs_read");
        assert!(end_seen, "expected successful ToolEnd for fs_read");
    }

    #[tokio::test]
    async fn run_streaming_announces_tool_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.txt");
        let provider = Arc::new(FakeProvider::new(vec![vec![
            tool_chunk(
                "fs_write",
                json!({ "path": path.to_string_lossy(), "content": "blocked" }),
            ),
            stop_chunk(),
        ]]));
        let runner = AgentRunner::new(provider, ToolRegistry::default_set(), "m");
        let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
        runner
            .run_streaming(vec![Message::new(Role::User, "write please")], tx)
            .await
            .unwrap();

        let events = drain(&mut rx);
        let rejected = events
            .iter()
            .any(|e| matches!(e, TurnEvent::ToolRejected { name, .. } if name == "fs_write"));
        assert!(rejected, "expected ToolRejected for fs_write");
    }

    #[test]
    fn auto_retrieve_formats_structured_results() {
        let output = json!({
            "results": [
                {"project": "infra", "source": "ref.md", "content": "alpha fact"},
                {"project": "ovelo", "source": "notes.md", "content": "beta fact"},
            ]
        });
        let block = format_retrieved_context(&output, 5).expect("context block");
        assert!(block.contains("projet infra / ref.md"));
        assert!(block.contains("alpha fact"));
        assert!(block.contains("beta fact"));
    }

    #[test]
    fn auto_retrieve_parses_json_string_payload() {
        // MCP servers commonly return their JSON as a single text-content string.
        let payload = json!({
            "results": [{"project": "p", "source": "s", "content": "gamma"}]
        })
        .to_string();
        let block = format_retrieved_context(&Value::String(payload), 5).expect("context block");
        assert!(block.contains("gamma"));
        assert!(block.contains("projet p / s"));
    }

    #[test]
    fn auto_retrieve_uses_plain_text_payload() {
        let output = Value::String("just some prose context".to_string());
        assert_eq!(
            format_retrieved_context(&output, 5).unwrap(),
            "just some prose context"
        );
    }

    #[test]
    fn auto_retrieve_top_k_limits_chunks() {
        let output = json!({
            "results": [
                {"project":"p","source":"a","content":"one"},
                {"project":"p","source":"b","content":"two"},
                {"project":"p","source":"c","content":"three"},
            ]
        });
        let block = format_retrieved_context(&output, 2).unwrap();
        assert!(block.contains("one") && block.contains("two"));
        assert!(!block.contains("three"), "top_k=2 must drop the 3rd chunk");
    }

    #[test]
    fn auto_retrieve_none_on_empty() {
        assert!(format_retrieved_context(&json!({"results": []}), 5).is_none());
        assert!(format_retrieved_context(&Value::String(String::new()), 5).is_none());
    }
}
