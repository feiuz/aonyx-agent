//! Ollama provider — JSON-lines streaming from `POST /api/chat`.
//!
//! Wire format: each JSON line is `{ "message": { "content": "..." }, "done": bool }`,
//! terminating on `done = true`. Unlike OpenAI, there is no SSE framing.

use aonyx_core::{AonyxError, ChatChunk, ChatRequest, ChatStream, LlmProvider, Result, Role};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

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

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

fn map_role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        // Ollama has no `tool` role; surface tool results as user-side context.
        Role::Tool => "user",
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let messages: Vec<OllamaMessage<'_>> = req
            .messages
            .iter()
            .map(|m| OllamaMessage {
                role: map_role(m.role),
                content: m.content.as_str(),
            })
            .collect();

        let mut payload = json!({
            "model": req.model,
            "messages": messages,
            "stream": true,
        });
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
        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| AonyxError::Provider(format!("ollama send: {e}")))?;

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
                    if let Some(c) = parse_line(&line) {
                        yield c;
                    }
                }
            }
            let trailing = buf.trim();
            if !trailing.is_empty() {
                if let Some(c) = parse_line(trailing) {
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
}

pub(crate) fn parse_line(line: &str) -> Option<ChatChunk> {
    if line.is_empty() {
        return None;
    }
    let chunk: OllamaChunk = serde_json::from_str(line).ok()?;
    let text = chunk.message.and_then(|m| m.content).unwrap_or_default();
    let finished = chunk.done;
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
    fn provider_name_is_ollama() {
        let p = OllamaProvider::new();
        assert_eq!(p.name(), "ollama");
        assert_eq!(p.base_url(), OLLAMA_DEFAULT_BASE_URL);
    }

    #[test]
    fn parses_content_line() {
        let line = r#"{"message":{"role":"assistant","content":"Hello"},"done":false}"#;
        let got = parse_line(line).expect("parsed");
        assert_eq!(got.delta_text, "Hello");
        assert!(!got.finished);
    }

    #[test]
    fn parses_terminal_line() {
        let line = r#"{"message":{"content":""},"done":true}"#;
        let got = parse_line(line).expect("parsed");
        assert!(got.finished);
        assert!(got.delta_text.is_empty());
    }

    #[test]
    fn ignores_empty_or_malformed_lines() {
        assert!(parse_line("").is_none());
        assert!(parse_line("not json").is_none());
    }

    #[test]
    fn ignores_empty_content_non_terminal() {
        let line = r#"{"message":{"content":""},"done":false}"#;
        assert!(parse_line(line).is_none());
    }
}
