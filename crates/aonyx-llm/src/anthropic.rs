//! Anthropic Claude provider — streaming over the Messages API.
//!
//! Endpoint: `POST {base_url}/v1/messages` with `stream: true`. The response is
//! `text/event-stream` (SSE); we parse `content_block_delta` (text deltas) and
//! `message_stop` (terminator) into [`ChatChunk`]s.
//!
//! Other event types (`message_start`, `content_block_start`,
//! `content_block_stop`, `message_delta`, `ping`) are intentionally ignored in
//! V1 — they carry metadata we don't need yet.

use aonyx_core::{AonyxError, ChatChunk, ChatRequest, ChatStream, LlmProvider, Result, Role};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Anthropic provider.
///
/// The API key is held as a `String` for V1 — we'll move to `secrecy::SecretString`
/// once the keyring integration lands (V1.2).
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

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: AnthropicContent<'a>,
}

/// `content` is either a single string (legacy + cheaper on the wire)
/// or an array of typed blocks — needed for vision (Phase S).
#[derive(Serialize)]
#[serde(untagged)]
enum AnthropicContent<'a> {
    Text(&'a str),
    Blocks(Vec<AnthropicBlock<'a>>),
}

/// One element of a multimodal content array.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicBlock<'a> {
    Text { text: &'a str },
    Image { source: AnthropicImageSource<'a> },
}

/// Anthropic vision sources are base64-encoded with a `type` discriminator.
#[derive(Serialize)]
struct AnthropicImageSource<'a> {
    #[serde(rename = "type")]
    source_type: &'static str,
    media_type: &'a str,
    data: &'a str,
}

fn map_role(role: Role) -> Option<&'static str> {
    match role {
        // `System` messages move to the top-level `system` field (handled by the caller).
        Role::System => None,
        Role::User => Some("user"),
        Role::Assistant => Some("assistant"),
        // V1 routes tool results through the `user` role (textual transcript).
        // V1.1 will emit proper `tool_use` / `tool_result` content blocks.
        Role::Tool => Some("user"),
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

        let messages: Vec<AnthropicMessage<'_>> = req
            .messages
            .iter()
            .filter_map(|m| {
                let role = map_role(m.role)?;
                // Phase S — when the message carries attachments, the
                // Anthropic API needs an array of typed content blocks
                // instead of a plain string. Text-only messages stay on
                // the cheaper string path.
                let content = if m.attachments.is_empty() {
                    AnthropicContent::Text(m.content.as_str())
                } else {
                    let mut blocks: Vec<AnthropicBlock<'_>> =
                        Vec::with_capacity(m.attachments.len() + 1);
                    for att in &m.attachments {
                        match att {
                            aonyx_core::Attachment::Image { media_type, data } => {
                                blocks.push(AnthropicBlock::Image {
                                    source: AnthropicImageSource {
                                        source_type: "base64",
                                        media_type,
                                        data,
                                    },
                                });
                            }
                        }
                    }
                    if !m.content.is_empty() {
                        blocks.push(AnthropicBlock::Text {
                            text: m.content.as_str(),
                        });
                    }
                    AnthropicContent::Blocks(blocks)
                };
                Some(AnthropicMessage { role, content })
            })
            .collect();

        let mut payload = json!({
            "model": req.model,
            "max_tokens": req.max_tokens.unwrap_or(2048),
            "messages": messages,
            "stream": true,
        });
        if !system_text.is_empty() {
            payload["system"] = json!(system_text);
        }
        if let Some(t) = req.temperature {
            payload["temperature"] = json!(t);
        }
        if !req.tools.is_empty() {
            payload["tools"] = json!(req.tools);
        }

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| AonyxError::Provider(format!("anthropic send: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AonyxError::Provider(format!("anthropic {status}: {body}")));
        }

        let byte_stream = response.bytes_stream();
        let chunk_stream = try_stream! {
            let mut buf = String::new();
            let mut stream = Box::pin(byte_stream);
            while let Some(item) = stream.next().await {
                let chunk = item.map_err(|e| AonyxError::Provider(format!("anthropic stream: {e}")))?;
                buf.push_str(std::str::from_utf8(&chunk).unwrap_or(""));

                // SSE events are separated by a blank line ("\n\n").
                while let Some(idx) = buf.find("\n\n") {
                    let block = buf[..idx].to_string();
                    buf.drain(..(idx + 2));
                    if let Some(parsed) = parse_sse_event(&block) {
                        yield parsed;
                    }
                }
            }
        };

        Ok(Box::pin(chunk_stream))
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicEvent {
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { delta: AnthropicDelta },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicDelta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(other)]
    Other,
}

/// Parse a single SSE block (one or more lines, with at least one `data:` line).
fn parse_sse_event(block: &str) -> Option<ChatChunk> {
    let mut data_parts = Vec::new();
    for line in block.lines() {
        if let Some(payload) = line.strip_prefix("data:") {
            data_parts.push(payload.trim_start());
        }
    }
    if data_parts.is_empty() {
        return None;
    }
    let data = data_parts.join("\n");
    let event: AnthropicEvent = serde_json::from_str(&data).ok()?;
    match event {
        AnthropicEvent::ContentBlockDelta {
            delta: AnthropicDelta::Text { text },
        } => Some(ChatChunk {
            delta_text: text,
            tool_call: None,
            finished: false,
        }),
        AnthropicEvent::MessageStop => Some(ChatChunk {
            delta_text: String::new(),
            tool_call: None,
            finished: true,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta_event() {
        let block = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        let got = parse_sse_event(block).expect("event parsed");
        assert_eq!(got.delta_text, "Hello");
        assert!(!got.finished);
        assert!(got.tool_call.is_none());
    }

    #[test]
    fn parses_message_stop_event() {
        let block = "event: message_stop\ndata: {\"type\":\"message_stop\"}";
        let got = parse_sse_event(block).expect("event parsed");
        assert!(got.delta_text.is_empty());
        assert!(got.finished);
    }

    #[test]
    fn ignores_message_start_event() {
        let block = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}";
        assert!(parse_sse_event(block).is_none());
    }

    #[test]
    fn ignores_ping_block_without_data_line() {
        let block = "event: ping";
        assert!(parse_sse_event(block).is_none());
    }

    #[test]
    fn ignores_non_text_delta() {
        let block = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"x\\\":1}\"}}";
        assert!(parse_sse_event(block).is_none());
    }

    #[test]
    fn provider_name_is_anthropic() {
        let p = AnthropicProvider::new("test-key");
        assert_eq!(p.name(), "anthropic");
    }

    #[test]
    fn with_base_url_overrides_default() {
        let p = AnthropicProvider::new("k").with_base_url("http://localhost:1234");
        assert_eq!(p.base_url, "http://localhost:1234");
    }
}
