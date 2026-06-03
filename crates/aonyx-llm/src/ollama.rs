//! Ollama provider — JSON-lines streaming from `POST /api/chat`.
//!
//! Wire format: each JSON line is `{ "message": { "content": ..., "tool_calls": [...] }, "done": bool }`,
//! terminating on `done = true`. Unlike OpenAI there is no SSE framing, and
//! tool calls arrive **complete** in one message (arguments as an object, no
//! call id) rather than as streamed fragments.

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

/// Default local Ollama server URL.
pub const OLLAMA_DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Ollama provider.
#[derive(Clone)]
pub struct OllamaProvider {
    client: Client,
    base_url: String,
}

impl OllamaProvider {
    /// Build a provider against the default local URL.
    pub fn new() -> Self {
        Self::with_base_url(OLLAMA_DEFAULT_BASE_URL)
    }

    /// Build a provider against a custom base URL (e.g. a remote Ollama deployment).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
        }
    }

    /// Inspect the configured base URL — handy for tests.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
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

/// Serialize one [`Message`] into an Ollama `/api/chat` message object.
fn build_message(m: &Message) -> Value {
    // Tool result — Ollama supports a `tool` role; it tracks no call id.
    if m.role == Role::Tool {
        return json!({ "role": "tool", "content": m.content });
    }
    // Assistant turn requesting tools — arguments ride as an *object*.
    if m.role == Role::Assistant && !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|tc| json!({ "function": { "name": tc.name, "arguments": tc.args } }))
            .collect();
        return json!({ "role": "assistant", "content": m.content, "tool_calls": calls });
    }
    // Plain text, with optional raw-base64 vision images (Ollama's shape).
    let images: Vec<&str> = m
        .attachments
        .iter()
        .map(|att| match att {
            Attachment::Image { data, .. } => data.as_str(),
        })
        .collect();
    if images.is_empty() {
        json!({ "role": map_role(m.role), "content": m.content })
    } else {
        json!({ "role": map_role(m.role), "content": m.content, "images": images })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let messages: Vec<Value> = req.messages.iter().map(build_message).collect();

        let mut payload = json!({
            "model": req.model,
            "messages": messages,
            "stream": true,
        });
        if !req.tools.is_empty() {
            // Ollama's `/api/chat` accepts the OpenAI function shape.
            payload["tools"] = json!(crate::openai_compat::translate_tools(&req.tools));
        }
        let mut options = serde_json::Map::new();
        if let Some(t) = req.temperature {
            options.insert("temperature".into(), json!(t));
        }
        if let Some(mt) = req.max_tokens {
            options.insert("num_predict".into(), json!(mt));
        }
        if !options.is_empty() {
            payload["options"] = json!(options);
        }

        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        // Phase RR — retry transient 429/5xx + network errors.
        let builder = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(payload.to_string());
        let response =
            crate::retry::send_with_retry(builder, crate::retry::RetryPolicy::default(), "ollama")
                .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AonyxError::Provider(format!("ollama {status}: {body}")));
        }

        let byte_stream = response.bytes_stream();
        let chunk_stream = try_stream! {
            let mut buf = String::new();
            let mut stream = Box::pin(byte_stream);
            while let Some(item) = stream.next().await {
                let bytes = item.map_err(|e| AonyxError::Provider(format!("ollama stream: {e}")))?;
                buf.push_str(std::str::from_utf8(&bytes).unwrap_or(""));
                while let Some(idx) = buf.find('\n') {
                    let line = buf[..idx].trim().to_string();
                    buf.drain(..(idx + 1));
                    for c in parse_line(&line) {
                        yield c;
                    }
                }
            }
            let trailing = buf.trim();
            if !trailing.is_empty() {
                for c in parse_line(trailing) {
                    yield c;
                }
            }
        };

        Ok(Box::pin(chunk_stream))
    }
}

#[derive(Deserialize)]
struct OllamaChunk {
    #[serde(default)]
    message: Option<OllamaMessageRecv>,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct OllamaMessageRecv {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Deserialize)]
struct OllamaToolCall {
    function: OllamaFn,
}

#[derive(Deserialize)]
struct OllamaFn {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// Parse one JSON line into zero or more [`ChatChunk`]s: a text delta, any
/// complete tool calls (Ollama batches them in one message), and/or the
/// terminal `done` marker.
pub(crate) fn parse_line(line: &str) -> Vec<ChatChunk> {
    if line.is_empty() {
        return Vec::new();
    }
    let chunk: OllamaChunk = match serde_json::from_str(line) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Some(msg) = chunk.message {
        if let Some(text) = msg.content {
            if !text.is_empty() {
                out.push(ChatChunk {
                    delta_text: text,
                    tool_call: None,
                    finished: false,
                });
            }
        }
        for (i, tc) in msg.tool_calls.into_iter().enumerate() {
            let args = if tc.function.arguments.is_null() {
                json!({})
            } else {
                tc.function.arguments
            };
            out.push(ChatChunk {
                delta_text: String::new(),
                tool_call: Some(ToolCall {
                    id: format!("call_{i}"),
                    name: tc.function.name,
                    args,
                }),
                finished: false,
            });
        }
    }
    if chunk.done {
        out.push(ChatChunk {
            delta_text: String::new(),
            tool_call: None,
            finished: true,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_name_is_ollama() {
        let p = OllamaProvider::new();
        assert_eq!(p.name(), "ollama");
        assert_eq!(p.base_url(), OLLAMA_DEFAULT_BASE_URL);
    }

    #[test]
    fn parses_content_line() {
        let line = r#"{"message":{"role":"assistant","content":"Hello"},"done":false}"#;
        let got = parse_line(line);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].delta_text, "Hello");
        assert!(!got[0].finished);
    }

    #[test]
    fn parses_terminal_line() {
        let got = parse_line(r#"{"message":{"content":""},"done":true}"#);
        assert_eq!(got.len(), 1);
        assert!(got[0].finished);
        assert!(got[0].delta_text.is_empty());
    }

    #[test]
    fn ignores_empty_or_malformed_lines() {
        assert!(parse_line("").is_empty());
        assert!(parse_line("not json").is_empty());
    }

    #[test]
    fn ignores_empty_content_non_terminal() {
        assert!(parse_line(r#"{"message":{"content":""},"done":false}"#).is_empty());
    }

    #[test]
    fn parses_tool_call_message() {
        let line = r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"list_projects","arguments":{"limit":5}}}]},"done":true}"#;
        let got = parse_line(line);
        // one tool-call chunk + the terminal done chunk
        assert_eq!(got.len(), 2);
        let tc = got[0].tool_call.as_ref().expect("tool call");
        assert_eq!(tc.name, "list_projects");
        assert_eq!(tc.args, json!({ "limit": 5 }));
        assert!(got[1].finished);
    }

    #[test]
    fn build_tool_result_message_uses_tool_role() {
        let v = build_message(&Message::tool_result("x", "result"));
        assert_eq!(v["role"], "tool");
        assert_eq!(v["content"], "result");
    }

    #[test]
    fn build_assistant_tool_calls_uses_object_arguments() {
        let call = ToolCall {
            id: "call_0".into(),
            name: "list_projects".into(),
            args: json!({ "limit": 5 }),
        };
        let v = build_message(&Message::assistant_tool_calls("", vec![call]));
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["tool_calls"][0]["function"]["name"], "list_projects");
        // arguments stay an object (unlike OpenAI's stringified form)
        assert_eq!(v["tool_calls"][0]["function"]["arguments"], json!({ "limit": 5 }));
    }

    #[test]
    fn build_text_message_omits_images() {
        let v = build_message(&Message::new(Role::User, "hi"));
        assert_eq!(v["content"], "hi");
        assert!(v.get("images").is_none());
    }
}
