//! Anthropic Claude provider — streaming over the Messages API.
//!
//! Endpoint: `POST {base_url}/v1/messages` with `stream: true`. The response
//! is `text/event-stream` (SSE). We parse text deltas (`text_delta`),
//! **tool calls** (`content_block_start` of type `tool_use` →
//! `input_json_delta` fragments → `content_block_stop`), and `message_stop`
//! (terminator) into [`ChatChunk`]s. Assistant tool-call turns and tool
//! results are replayed as `tool_use` / `tool_result` content blocks.

use std::collections::HashMap;

use aonyx_core::{
    Attachment, AonyxError, ChatChunk, ChatRequest, ChatStream, LlmProvider, Message, Result, Role,
    ToolCall,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Anthropic provider.
#[derive(Clone)]
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    /// Build a new Anthropic provider with the default API base URL.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            base_url: ANTHROPIC_BASE_URL.to_string(),
        }
    }

    /// Override the base URL — used by integration tests against a mock server.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

/// Build the Anthropic `system` field for a (possibly empty) system prompt.
/// Returns `None` for an empty prompt; else a single cached `text` block.
fn build_system_field(system_text: &str) -> Option<Value> {
    if system_text.is_empty() {
        return None;
    }
    Some(json!([{
        "type": "text",
        "text": system_text,
        "cache_control": { "type": "ephemeral" },
    }]))
}

/// Serialize one [`Message`] into an Anthropic message object. `System`
/// messages return `None` — they are hoisted into the top-level `system`
/// field by the caller.
fn build_message(m: &Message) -> Option<Value> {
    match m.role {
        Role::System => None,
        // Tool results ride back as a `tool_result` block in a user message.
        Role::Tool => Some(json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content,
            }],
        })),
        // Assistant turn that requested tools: text (if any) + tool_use blocks.
        Role::Assistant if !m.tool_calls.is_empty() => {
            let mut blocks: Vec<Value> = Vec::new();
            if !m.content.is_empty() {
                blocks.push(json!({ "type": "text", "text": m.content }));
            }
            for tc in &m.tool_calls {
                blocks.push(json!({
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.name,
                    "input": tc.args,
                }));
            }
            Some(json!({ "role": "assistant", "content": blocks }))
        }
        role => {
            let role_str = if role == Role::Assistant {
                "assistant"
            } else {
                "user"
            };
            if m.attachments.is_empty() {
                Some(json!({ "role": role_str, "content": m.content }))
            } else {
                let mut blocks: Vec<Value> = Vec::with_capacity(m.attachments.len() + 1);
                for att in &m.attachments {
                    match att {
                        Attachment::Image { media_type, data } => blocks.push(json!({
                            "type": "image",
                            "source": { "type": "base64", "media_type": media_type, "data": data },
                        })),
                    }
                }
                if !m.content.is_empty() {
                    blocks.push(json!({ "type": "text", "text": m.content }));
                }
                Some(json!({ "role": role_str, "content": blocks }))
            }
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let system_text: String = req
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let messages: Vec<Value> = req.messages.iter().filter_map(build_message).collect();

        let mut payload = json!({
            "model": req.model,
            "max_tokens": req.max_tokens.unwrap_or(2048),
            "messages": messages,
            "stream": true,
        });
        if let Some(system_field) = build_system_field(&system_text) {
            payload["system"] = system_field;
        }
        if let Some(t) = req.temperature {
            payload["temperature"] = json!(t);
        }
        // The runner already emits tools in Anthropic's
        // `{name, description, input_schema}` shape — forward as-is.
        if !req.tools.is_empty() {
            payload["tools"] = json!(req.tools);
        }

        let builder = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .body(payload.to_string());
        let response =
            crate::retry::send_with_retry(builder, crate::retry::RetryPolicy::default(), "anthropic")
                .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AonyxError::Provider(format!("anthropic {status}: {body}")));
        }

        let byte_stream = response.bytes_stream();
        let chunk_stream = try_stream! {
            let mut buf = String::new();
            let mut acc = AnthropicAccumulator::default();
            let mut stream = Box::pin(byte_stream);
            while let Some(item) = stream.next().await {
                let chunk = item.map_err(|e| AonyxError::Provider(format!("anthropic stream: {e}")))?;
                buf.push_str(std::str::from_utf8(&chunk).unwrap_or(""));
                while let Some(idx) = buf.find("\n\n") {
                    let block = buf[..idx].to_string();
                    buf.drain(..(idx + 2));
                    for parsed in acc.push_block(&block) {
                        yield parsed;
                    }
                }
            }
        };

        Ok(Box::pin(chunk_stream))
    }
}

// ---- streaming SSE parse + tool_use accumulation -----------------------

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "content_block_start")]
    Start {
        index: usize,
        content_block: BlockStart,
    },
    #[serde(rename = "content_block_delta")]
    Delta { index: usize, delta: DeltaKind },
    #[serde(rename = "content_block_stop")]
    Stop { index: usize },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum BlockStart {
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum DeltaKind {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(rename = "input_json_delta")]
    InputJson { partial_json: String },
    #[serde(other)]
    Other,
}

struct PartialToolUse {
    id: String,
    name: String,
    args: String,
}

/// Accumulates SSE blocks into [`ChatChunk`]s, buffering `tool_use` blocks
/// (by content-block `index`) until their `content_block_stop`.
#[derive(Default)]
struct AnthropicAccumulator {
    blocks: HashMap<usize, PartialToolUse>,
}

impl AnthropicAccumulator {
    fn push_block(&mut self, block: &str) -> Vec<ChatChunk> {
        let Some(data) = extract_data(block) else {
            return Vec::new();
        };
        let event: Event = match serde_json::from_str(&data) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        match event {
            Event::Start {
                index,
                content_block: BlockStart::ToolUse { id, name },
            } => {
                self.blocks.insert(
                    index,
                    PartialToolUse {
                        id,
                        name,
                        args: String::new(),
                    },
                );
                Vec::new()
            }
            Event::Start { .. } => Vec::new(),
            Event::Delta {
                delta: DeltaKind::Text { text },
                ..
            } => vec![ChatChunk {
                delta_text: text,
                tool_call: None,
                finished: false,
            }],
            Event::Delta {
                index,
                delta: DeltaKind::InputJson { partial_json },
            } => {
                if let Some(b) = self.blocks.get_mut(&index) {
                    b.args.push_str(&partial_json);
                }
                Vec::new()
            }
            Event::Delta { .. } => Vec::new(),
            Event::Stop { index } => match self.blocks.remove(&index) {
                Some(b) => {
                    let args =
                        serde_json::from_str::<Value>(&b.args).unwrap_or_else(|_| json!({}));
                    vec![ChatChunk {
                        delta_text: String::new(),
                        tool_call: Some(ToolCall {
                            id: b.id,
                            name: b.name,
                            args,
                        }),
                        finished: false,
                    }]
                }
                None => Vec::new(),
            },
            Event::MessageStop => vec![ChatChunk {
                delta_text: String::new(),
                tool_call: None,
                finished: true,
            }],
            Event::Other => Vec::new(),
        }
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
        AnthropicAccumulator::default().push_block(block)
    }

    #[test]
    fn parses_text_delta_event() {
        let got = one("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].delta_text, "Hello");
        assert!(!got[0].finished);
        assert!(got[0].tool_call.is_none());
    }

    #[test]
    fn parses_message_stop_event() {
        let got = one("event: message_stop\ndata: {\"type\":\"message_stop\"}");
        assert_eq!(got.len(), 1);
        assert!(got[0].finished);
    }

    #[test]
    fn ignores_message_start_event() {
        let got = one("data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}");
        assert!(got.is_empty());
    }

    #[test]
    fn ignores_block_without_data_line() {
        assert!(one("event: ping").is_empty());
    }

    #[test]
    fn accumulates_tool_use_block() {
        let mut acc = AnthropicAccumulator::default();
        assert!(acc
            .push_block("data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"list_projects\"}}")
            .is_empty());
        assert!(acc
            .push_block("data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"limit\\\":\"}}")
            .is_empty());
        assert!(acc
            .push_block("data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"5}\"}}")
            .is_empty());
        let out = acc.push_block("data: {\"type\":\"content_block_stop\",\"index\":0}");
        assert_eq!(out.len(), 1);
        let tc = out[0].tool_call.as_ref().expect("tool call");
        assert_eq!(tc.id, "toolu_1");
        assert_eq!(tc.name, "list_projects");
        assert_eq!(tc.args, json!({ "limit": 5 }));
    }

    #[test]
    fn build_tool_result_message() {
        let v = build_message(&Message::tool_result("toolu_1", "result text")).expect("some");
        assert_eq!(v["role"], "user");
        assert_eq!(v["content"][0]["type"], "tool_result");
        assert_eq!(v["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(v["content"][0]["content"], "result text");
    }

    #[test]
    fn build_assistant_tool_use_message() {
        let call = ToolCall {
            id: "toolu_1".into(),
            name: "list_projects".into(),
            args: json!({ "limit": 5 }),
        };
        let v = build_message(&Message::assistant_tool_calls("let me check", vec![call])).expect("some");
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["content"][0]["type"], "text");
        assert_eq!(v["content"][0]["text"], "let me check");
        assert_eq!(v["content"][1]["type"], "tool_use");
        assert_eq!(v["content"][1]["id"], "toolu_1");
        assert_eq!(v["content"][1]["name"], "list_projects");
        assert_eq!(v["content"][1]["input"], json!({ "limit": 5 }));
    }

    #[test]
    fn system_message_is_hoisted_out() {
        assert!(build_message(&Message::new(Role::System, "be brief")).is_none());
    }

    #[test]
    fn system_field_carries_cache_control() {
        let v = build_system_field("be brief").expect("some");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["text"], "be brief");
        assert_eq!(arr[0]["cache_control"]["type"], "ephemeral");
        assert!(build_system_field("").is_none());
    }

    #[test]
    fn provider_name_is_anthropic() {
        assert_eq!(AnthropicProvider::new("k").name(), "anthropic");
    }

    #[test]
    fn with_base_url_overrides_default() {
        let p = AnthropicProvider::new("k").with_base_url("http://localhost:1234");
        assert_eq!(p.base_url, "http://localhost:1234");
    }
}
