//! OpenAI Chat Completions compatible providers.
//!
//! Used as the backend for OpenAI, OpenRouter, LM Studio, and any other
//! "speaks-OpenAI" endpoint. They share:
//!
//! - `POST {base_url}/v1/chat/completions`
//! - Bearer auth (`Authorization: Bearer {api_key}`), omitted when empty.
//! - SSE wire protocol with a `data: [DONE]` terminator.
//! - Per-event JSON shaped as `{ choices: [{ delta: { content: ... } }] }`.
//!
//! Tool-call streaming (function-call delta accumulation) is deliberately
//! deferred to P3 — V1 only surfaces text deltas.

use aonyx_core::{AonyxError, ChatChunk, ChatRequest, ChatStream, LlmProvider, Result, Role};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Generic OpenAI-compatible provider — paramétré by base URL, name, headers.
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

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: OpenAiContent<'a>,
}

/// Either a plain text body (the legacy + cheap path) or an array of
/// content blocks — the latter is needed for vision (Phase T).
#[derive(Serialize)]
#[serde(untagged)]
enum OpenAiContent<'a> {
    Text(&'a str),
    Blocks(Vec<OpenAiBlock<'a>>),
}

/// One element of a multimodal content array. OpenAI uses
/// `image_url` (with a nested `{url: "data:..."}` object) for vision,
/// distinct from Anthropic's `image` / `source` shape.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAiBlock<'a> {
    Text {
        text: &'a str,
    },
    #[serde(rename = "image_url")]
    ImageUrl {
        image_url: OpenAiImageUrl,
    },
}

/// `image_url` carrier — OpenAI expects either a remote https:// URL or
/// a `data:image/...;base64,XXX` blob. We always emit the latter so the
/// adapter doesn't have to host the bytes anywhere.
#[derive(Serialize)]
struct OpenAiImageUrl {
    url: String,
}

fn map_role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        // V1 surfaces tool results as `tool` role; full tool_call_id wiring lands in P3.
        Role::Tool => "tool",
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        self.provider_name
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let messages: Vec<OpenAiMessage<'_>> = req
            .messages
            .iter()
            .map(|m| {
                // Phase T — vision-capable models accept an array of
                // content blocks. Text-only messages keep the cheap
                // single-string path; whenever the message has
                // attachments we emit `[image_url..., text]`.
                let content = if m.attachments.is_empty() {
                    OpenAiContent::Text(m.content.as_str())
                } else {
                    let mut blocks: Vec<OpenAiBlock<'_>> =
                        Vec::with_capacity(m.attachments.len() + 1);
                    for att in &m.attachments {
                        match att {
                            aonyx_core::Attachment::Image { media_type, data } => {
                                blocks.push(OpenAiBlock::ImageUrl {
                                    image_url: OpenAiImageUrl {
                                        url: format!("data:{media_type};base64,{data}"),
                                    },
                                });
                            }
                        }
                    }
                    if !m.content.is_empty() {
                        blocks.push(OpenAiBlock::Text {
                            text: m.content.as_str(),
                        });
                    }
                    OpenAiContent::Blocks(blocks)
                };
                OpenAiMessage {
                    role: map_role(m.role),
                    content,
                }
            })
            .collect();

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

        let response = rb
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| AonyxError::Provider(format!("{} send: {e}", self.provider_name)))?;

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
            let mut stream = Box::pin(byte_stream);
            while let Some(item) = stream.next().await {
                let bytes = item.map_err(|e| AonyxError::Provider(format!("{provider_name} stream: {e}")))?;
                buf.push_str(std::str::from_utf8(&bytes).unwrap_or(""));
                while let Some(idx) = buf.find("\n\n") {
                    let block = buf[..idx].to_string();
                    buf.drain(..(idx + 2));
                    if let Some(c) = parse_sse_block(&block) {
                        yield c;
                    }
                }
            }
        };

        Ok(Box::pin(chunk_stream))
    }
}

#[derive(Deserialize)]
struct OpenAiSseChunk {
    #[serde(default)]
    choices: Vec<OpenAiSseChoice>,
}

#[derive(Deserialize)]
struct OpenAiSseChoice {
    #[serde(default)]
    delta: OpenAiDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
}

/// Parse a single SSE block (one or more lines, at least one `data:`).
///
/// `data: [DONE]` yields a terminal chunk with `finished = true`.
pub(crate) fn parse_sse_block(block: &str) -> Option<ChatChunk> {
    let mut data_parts = Vec::new();
    for line in block.lines() {
        if let Some(p) = line.strip_prefix("data:") {
            data_parts.push(p.trim_start());
        }
    }
    if data_parts.is_empty() {
        return None;
    }
    let data = data_parts.join("\n");
    if data == "[DONE]" {
        return Some(ChatChunk {
            delta_text: String::new(),
            tool_call: None,
            finished: true,
        });
    }
    let chunk: OpenAiSseChunk = serde_json::from_str(&data).ok()?;
    let choice = chunk.choices.into_iter().next()?;
    let text = choice.delta.content.unwrap_or_default();
    let finished = choice.finish_reason.is_some();
    if text.is_empty() && !finished {
        return None;
    }
    Some(ChatChunk {
        delta_text: text,
        tool_call: None,
        finished,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_delta() {
        let block = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}";
        let got = parse_sse_block(block).expect("event parsed");
        assert_eq!(got.delta_text, "Hello");
        assert!(!got.finished);
    }

    #[test]
    fn parses_done_marker() {
        let block = "data: [DONE]";
        let got = parse_sse_block(block).expect("done parsed");
        assert!(got.finished);
        assert!(got.delta_text.is_empty());
    }

    #[test]
    fn parses_finish_reason_chunk() {
        let block = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}";
        let got = parse_sse_block(block).expect("finish parsed");
        assert!(got.finished);
        assert!(got.delta_text.is_empty());
    }

    #[test]
    fn ignores_empty_chunk_without_content_or_finish() {
        let block = "data: {\"choices\":[{\"delta\":{}}]}";
        assert!(parse_sse_block(block).is_none());
    }

    #[test]
    fn ignores_blocks_without_data_line() {
        let block = "event: ping";
        assert!(parse_sse_block(block).is_none());
    }

    #[test]
    fn ignores_malformed_json() {
        let block = "data: { this is not json";
        assert!(parse_sse_block(block).is_none());
    }

    #[test]
    fn text_only_message_serialises_as_plain_string_content() {
        let m = OpenAiMessage {
            role: "user",
            content: OpenAiContent::Text("hello"),
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "user");
        assert_eq!(v["content"], "hello");
    }

    #[test]
    fn vision_message_serialises_as_image_url_blocks() {
        let m = OpenAiMessage {
            role: "user",
            content: OpenAiContent::Blocks(vec![
                OpenAiBlock::ImageUrl {
                    image_url: OpenAiImageUrl {
                        url: "data:image/png;base64,AAAA".into(),
                    },
                },
                OpenAiBlock::Text { text: "describe" },
            ]),
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "user");
        let blocks = v["content"].as_array().expect("array");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "image_url");
        assert_eq!(
            blocks[0]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
        assert_eq!(blocks[1]["type"], "text");
        assert_eq!(blocks[1]["text"], "describe");
    }
}
