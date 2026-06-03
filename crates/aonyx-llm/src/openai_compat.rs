//! OpenAI Chat Completions compatible providers.
//!
//! Backend for OpenAI, OpenRouter, LM Studio, Ollama, llama.cpp's
//! `llama-server`, and any other "speaks-OpenAI" endpoint. They share:
//!
//! - `POST {base_url}/v1/chat/completions`
//! - Bearer auth (`Authorization: Bearer {api_key}`), omitted when empty.
//! - SSE wire protocol with a `data: [DONE]` terminator.
//! - Per-event JSON shaped as `{ choices: [{ delta: { content | tool_calls } }] }`.
//!
//! **Tool calling** is wired end-to-end: the request advertises tools in
//! OpenAI's `{type:"function", function:{...}}` shape, assistant tool-call
//! turns and `tool` results are replayed with `tool_call_id`, and the
//! streamed `delta.tool_calls[]` fragments are accumulated (by `index`) into
//! a [`ToolCall`] emitted when the turn finishes.

use aonyx_core::{
    AonyxError, Attachment, ChatChunk, ChatRequest, ChatStream, LlmProvider, Message, Result, Role,
    ToolCall,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

/// Generic OpenAI-compatible provider — parameterised by base URL, name, headers.
#[derive(Clone)]
pub struct OpenAiCompatProvider {
    provider_name: &'static str,
    client: Client,
    api_key: String,
    base_url: String,
    extra_headers: Vec<(String, String)>,
}

impl OpenAiCompatProvider {
    /// Build a new compat provider.
    ///
    /// `api_key` may be empty for local endpoints (LM Studio, llama.cpp) that
    /// do not require auth; in that case the `Authorization` header is omitted.
    pub fn new(
        provider_name: &'static str,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            provider_name,
            client: Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
            extra_headers: Vec::new(),
        }
    }

    /// Attach an extra header (e.g. `HTTP-Referer` for OpenRouter).
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }

    /// Inspect the configured base URL — handy for tests.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

fn map_role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

/// Serialize one [`Message`] into an OpenAI chat message object, handling
/// tool-call requests, tool results, and vision attachments.
fn build_message(m: &Message) -> Value {
    // A tool result: `{role:"tool", tool_call_id, content}`.
    if m.role == Role::Tool {
        return json!({
            "role": "tool",
            "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
            "content": m.content,
        });
    }

    // An assistant turn that requested tools: content + `tool_calls[]`.
    if m.role == Role::Assistant && !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|tc| {
                json!({
                    "id": tc.id,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        // OpenAI wants the arguments as a JSON-encoded string.
                        "arguments": tc.args.to_string(),
                    },
                })
            })
            .collect();
        return json!({
            "role": "assistant",
            "content": if m.content.is_empty() { Value::Null } else { json!(m.content) },
            "tool_calls": calls,
        });
    }

    // Vision: an array of content blocks.
    if !m.attachments.is_empty() {
        let mut blocks: Vec<Value> = Vec::with_capacity(m.attachments.len() + 1);
        for att in &m.attachments {
            match att {
                Attachment::Image { media_type, data } => blocks.push(json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{media_type};base64,{data}") },
                })),
            }
        }
        if !m.content.is_empty() {
            blocks.push(json!({ "type": "text", "text": m.content }));
        }
        return json!({ "role": map_role(m.role), "content": blocks });
    }

    // Plain text.
    json!({ "role": map_role(m.role), "content": m.content })
}

/// Translate the runner's Anthropic-shaped tool schemas
/// (`{name, description, input_schema}`) into OpenAI's function shape.
/// Shared with the Ollama provider, whose `/api/chat` accepts the same shape.
pub(crate) fn translate_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "parameters": t
                        .get("input_schema")
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                },
            })
        })
        .collect()
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        self.provider_name
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let messages: Vec<Value> = req.messages.iter().map(build_message).collect();

        let mut payload = json!({
            "model": req.model,
            "messages": messages,
            "stream": true,
        });
        if let Some(t) = req.temperature {
            payload["temperature"] = json!(t);
        }
        if let Some(mt) = req.max_tokens {
            payload["max_tokens"] = json!(mt);
        }
        if !req.tools.is_empty() {
            payload["tools"] = json!(translate_tools(&req.tools));
        }

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let mut rb = self
            .client
            .post(&url)
            .header("content-type", "application/json");
        if !self.api_key.is_empty() {
            rb = rb.header("authorization", format!("Bearer {}", self.api_key));
        }
        for (k, v) in &self.extra_headers {
            rb = rb.header(k.as_str(), v.as_str());
        }

        // Phase RR — retry transient 429/5xx + network errors.
        let response = crate::retry::send_with_retry(
            rb.body(payload.to_string()),
            crate::retry::RetryPolicy::default(),
            self.provider_name,
        )
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AonyxError::Provider(format!(
                "{} {status}: {body}",
                self.provider_name
            )));
        }

        let byte_stream = response.bytes_stream();
        let provider_name = self.provider_name;
        let chunk_stream = try_stream! {
            let mut buf = String::new();
            let mut acc = SseAccumulator::default();
            let mut stream = Box::pin(byte_stream);
            while let Some(item) = stream.next().await {
                let bytes = item.map_err(|e| AonyxError::Provider(format!("{provider_name} stream: {e}")))?;
                buf.push_str(std::str::from_utf8(&bytes).unwrap_or(""));
                while let Some(idx) = buf.find("\n\n") {
                    let block = buf[..idx].to_string();
                    buf.drain(..(idx + 2));
                    for chunk in acc.push_block(&block) {
                        yield chunk;
                    }
                }
            }
        };

        Ok(Box::pin(chunk_stream))
    }
}

// ---- streaming SSE parse + tool-call accumulation ----------------------

#[derive(Deserialize)]
struct SseChunk {
    #[serde(default)]
    choices: Vec<SseChoice>,
}

#[derive(Deserialize)]
struct SseChoice {
    #[serde(default)]
    delta: SseDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct SseDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<SseToolCallDelta>,
}

#[derive(Deserialize)]
struct SseToolCallDelta {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<SseFn>,
}

#[derive(Deserialize)]
struct SseFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// One tool call being assembled across streamed fragments.
#[derive(Default, Clone)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

/// Accumulates SSE blocks into [`ChatChunk`]s, buffering streamed
/// `tool_calls[]` fragments by `index` until the turn finishes.
#[derive(Default)]
struct SseAccumulator {
    calls: Vec<PartialCall>,
    flushed: bool,
}

impl SseAccumulator {
    /// Feed one SSE block; return the chunks to yield for it.
    fn push_block(&mut self, block: &str) -> Vec<ChatChunk> {
        let Some(data) = extract_data(block) else {
            return Vec::new();
        };
        if data == "[DONE]" {
            let mut out = self.flush();
            out.push(finished_chunk());
            return out;
        }
        let parsed: SseChunk = match serde_json::from_str(&data) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let Some(choice) = parsed.choices.into_iter().next() else {
            return Vec::new();
        };

        let mut out = Vec::new();
        if let Some(text) = choice.delta.content {
            if !text.is_empty() {
                out.push(ChatChunk {
                    delta_text: text,
                    tool_call: None,
                    finished: false,
                });
            }
        }
        for d in choice.delta.tool_calls {
            if self.calls.len() <= d.index {
                self.calls.resize(d.index + 1, PartialCall::default());
            }
            let slot = &mut self.calls[d.index];
            if let Some(id) = d.id {
                if !id.is_empty() {
                    slot.id = id;
                }
            }
            if let Some(f) = d.function {
                if let Some(n) = f.name {
                    if !n.is_empty() {
                        slot.name = n;
                    }
                }
                if let Some(a) = f.arguments {
                    slot.args.push_str(&a);
                }
            }
        }
        if choice.finish_reason.is_some() {
            out.extend(self.flush());
            out.push(finished_chunk());
        }
        out
    }

    /// Emit one tool-call chunk per accumulated call (once).
    fn flush(&mut self) -> Vec<ChatChunk> {
        if self.flushed {
            return Vec::new();
        }
        self.flushed = true;
        std::mem::take(&mut self.calls)
            .into_iter()
            .enumerate()
            .filter(|(_, p)| !p.name.is_empty())
            .map(|(i, p)| {
                let args = serde_json::from_str::<Value>(&p.args).unwrap_or_else(|_| json!({}));
                let id = if p.id.is_empty() {
                    format!("call_{i}")
                } else {
                    p.id
                };
                ChatChunk {
                    delta_text: String::new(),
                    tool_call: Some(ToolCall {
                        id,
                        name: p.name,
                        args,
                    }),
                    finished: false,
                }
            })
            .collect()
    }
}

fn finished_chunk() -> ChatChunk {
    ChatChunk {
        delta_text: String::new(),
        tool_call: None,
        finished: true,
    }
}

/// Join the `data:` lines of an SSE block, or `None` when there are none.
fn extract_data(block: &str) -> Option<String> {
    let mut data = String::new();
    let mut found = false;
    for line in block.lines() {
        if let Some(p) = line.strip_prefix("data:") {
            if found {
                data.push('\n');
            }
            data.push_str(p.trim_start());
            found = true;
        }
    }
    found.then_some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(block: &str) -> Vec<ChatChunk> {
        SseAccumulator::default().push_block(block)
    }

    #[test]
    fn parses_content_delta() {
        let got = one("data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].delta_text, "Hello");
        assert!(!got[0].finished);
    }

    #[test]
    fn parses_done_marker() {
        let got = one("data: [DONE]");
        assert_eq!(got.len(), 1);
        assert!(got[0].finished);
    }

    #[test]
    fn parses_finish_reason_chunk() {
        let got = one("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}");
        assert_eq!(got.len(), 1);
        assert!(got[0].finished);
    }

    #[test]
    fn ignores_empty_and_malformed() {
        assert!(one("data: {\"choices\":[{\"delta\":{}}]}").is_empty());
        assert!(one("event: ping").is_empty());
        assert!(one("data: { not json").is_empty());
    }

    #[test]
    fn accumulates_streamed_tool_call() {
        // Mirrors llama-server / OpenAI: id+name first, arguments in fragments,
        // then a finish_reason terminates the turn.
        let mut acc = SseAccumulator::default();
        assert!(acc
            .push_block(
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_abc\",\"function\":{\"name\":\"list_projects\",\"arguments\":\"\"}}]}}]}"
            )
            .is_empty());
        assert!(acc
            .push_block("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"limit\\\":\"}}]}}]}")
            .is_empty());
        assert!(acc
            .push_block("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"5}\"}}]}}]}")
            .is_empty());
        let out =
            acc.push_block("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}");
        // one tool-call chunk + one finished chunk
        assert_eq!(out.len(), 2);
        let tc = out[0].tool_call.as_ref().expect("tool call");
        assert_eq!(tc.id, "call_abc");
        assert_eq!(tc.name, "list_projects");
        assert_eq!(tc.args, json!({ "limit": 5 }));
        assert!(out[1].finished);
    }

    #[test]
    fn translate_tools_to_openai_function_shape() {
        let anthropic = vec![json!({
            "name": "list_projects",
            "description": "List RAG projects",
            "input_schema": { "type": "object", "properties": {} },
        })];
        let oai = translate_tools(&anthropic);
        assert_eq!(oai[0]["type"], "function");
        assert_eq!(oai[0]["function"]["name"], "list_projects");
        assert_eq!(oai[0]["function"]["description"], "List RAG projects");
        assert_eq!(oai[0]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn build_text_message() {
        let v = build_message(&Message::new(Role::User, "hi"));
        assert_eq!(v["role"], "user");
        assert_eq!(v["content"], "hi");
    }

    #[test]
    fn build_tool_result_message() {
        let v = build_message(&Message::tool_result("call_abc", "{\"ok\":true}"));
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "call_abc");
        assert_eq!(v["content"], "{\"ok\":true}");
    }

    #[test]
    fn build_assistant_tool_calls_message() {
        let call = ToolCall {
            id: "call_abc".into(),
            name: "list_projects".into(),
            args: json!({ "limit": 5 }),
        };
        let v = build_message(&Message::assistant_tool_calls("", vec![call]));
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["tool_calls"][0]["id"], "call_abc");
        assert_eq!(v["tool_calls"][0]["type"], "function");
        assert_eq!(v["tool_calls"][0]["function"]["name"], "list_projects");
        // arguments must be a JSON-encoded *string*
        assert_eq!(v["tool_calls"][0]["function"]["arguments"], "{\"limit\":5}");
    }
}
