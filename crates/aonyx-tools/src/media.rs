//! Multimodal generation tools (Phase ZZ) — `image_gen` and `tts` over an
//! OpenAI-compatible API.
//!
//! Lightweight (just `reqwest` + `base64`). The OpenAI key is injected by
//! the binary (resolved config → keyring → env), so the tools work whether
//! the key lives in the OS keyring or the environment. With no key they
//! return a clear error rather than failing the turn.

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_BASE: &str = "https://api.openai.com";

fn base(base_url: &Option<String>) -> String {
    base_url
        .clone()
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// `image_gen` — generate an image from a text prompt via the OpenAI
/// images API; saves a PNG and returns its path.
pub struct ImageGen {
    api_key: Option<String>,
    base_url: String,
    model: String,
}

impl ImageGen {
    /// Build with an optional API key and base URL (defaults to OpenAI).
    pub fn new(api_key: Option<String>, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base(&base_url),
            model: "gpt-image-1".to_string(),
        }
    }
}

#[derive(Deserialize)]
struct ImageArgs {
    prompt: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl ToolHandler for ImageGen {
    fn name(&self) -> &str {
        "image_gen"
    }
    fn classify(&self) -> SafetyClass {
        SafetyClass::Caution
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Generate an image from a text prompt (OpenAI images API); saves a PNG and returns its path.",
            "properties": {
                "prompt": { "type": "string" },
                "size": { "type": "string", "description": "e.g. 1024x1024 (default), 1536x1024, 1024x1536" },
                "path": { "type": "string", "description": "output PNG path (default ./aonyx-image.png)" }
            },
            "required": ["prompt"]
        })
    }
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: ImageArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("image_gen args: {e}")))?;
        let key = self.api_key.clone().ok_or_else(|| {
            AonyxError::Tool(
                "image_gen: no OpenAI key — run `aonyx setup` or export OPENAI_API_KEY".into(),
            )
        })?;
        let body = json!({
            "model": self.model,
            "prompt": args.prompt,
            "n": 1,
            "size": args.size.unwrap_or_else(|| "1024x1024".to_string()),
        });
        let resp = reqwest::Client::new()
            .post(format!("{}/v1/images/generations", self.base_url))
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AonyxError::Tool(format!("image_gen request: {e}")))?;
        let status = resp.status();
        let val: Value = resp
            .json()
            .await
            .map_err(|e| AonyxError::Tool(format!("image_gen decode: {e}")))?;
        if !status.is_success() {
            let msg = val
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(AonyxError::Tool(format!("image_gen API {status}: {msg}")));
        }
        let datum = val
            .pointer("/data/0")
            .ok_or_else(|| AonyxError::Tool("image_gen: empty response".into()))?;
        let bytes: Vec<u8> = if let Some(b64) = datum.get("b64_json").and_then(|b| b.as_str()) {
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| AonyxError::Tool(format!("image_gen b64: {e}")))?
        } else if let Some(img_url) = datum.get("url").and_then(|u| u.as_str()) {
            reqwest::get(img_url)
                .await
                .map_err(|e| AonyxError::Tool(format!("image_gen fetch: {e}")))?
                .bytes()
                .await
                .map_err(|e| AonyxError::Tool(format!("image_gen fetch body: {e}")))?
                .to_vec()
        } else {
            return Err(AonyxError::Tool(
                "image_gen: response had neither b64_json nor url".into(),
            ));
        };
        let path = args.path.unwrap_or_else(|| "aonyx-image.png".to_string());
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| AonyxError::Tool(format!("image_gen write {path}: {e}")))?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "path": path, "bytes": bytes.len() }),
            error: None,
        })
    }
}

/// `tts` — synthesize speech from text via the OpenAI audio/speech API;
/// saves an MP3 and returns its path.
pub struct Tts {
    api_key: Option<String>,
    base_url: String,
    model: String,
}

impl Tts {
    /// Build with an optional API key and base URL (defaults to OpenAI).
    pub fn new(api_key: Option<String>, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base(&base_url),
            model: "gpt-4o-mini-tts".to_string(),
        }
    }
}

#[derive(Deserialize)]
struct TtsArgs {
    text: String,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl ToolHandler for Tts {
    fn name(&self) -> &str {
        "tts"
    }
    fn classify(&self) -> SafetyClass {
        SafetyClass::Caution
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Synthesize speech from text (OpenAI audio/speech); saves an MP3 and returns its path.",
            "properties": {
                "text": { "type": "string" },
                "voice": { "type": "string", "description": "alloy (default), echo, fable, onyx, nova, shimmer, …" },
                "path": { "type": "string", "description": "output MP3 path (default ./aonyx-speech.mp3)" }
            },
            "required": ["text"]
        })
    }
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: TtsArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("tts args: {e}")))?;
        let key = self.api_key.clone().ok_or_else(|| {
            AonyxError::Tool(
                "tts: no OpenAI key — run `aonyx setup` or export OPENAI_API_KEY".into(),
            )
        })?;
        let body = json!({
            "model": self.model,
            "input": args.text,
            "voice": args.voice.unwrap_or_else(|| "alloy".to_string()),
            "response_format": "mp3",
        });
        let resp = reqwest::Client::new()
            .post(format!("{}/v1/audio/speech", self.base_url))
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AonyxError::Tool(format!("tts request: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(AonyxError::Tool(format!("tts API {status}: {txt}")));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AonyxError::Tool(format!("tts body: {e}")))?;
        let path = args.path.unwrap_or_else(|| "aonyx-speech.mp3".to_string());
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| AonyxError::Tool(format!("tts write {path}: {e}")))?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "path": path, "bytes": bytes.len() }),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_stable() {
        assert_eq!(ImageGen::new(None, None).name(), "image_gen");
        assert_eq!(Tts::new(None, None).name(), "tts");
    }

    #[test]
    fn base_url_defaults_and_trims() {
        assert_eq!(base(&None), "https://api.openai.com");
        assert_eq!(base(&Some("http://x/".to_string())), "http://x");
    }

    #[tokio::test]
    async fn image_gen_without_key_errors_without_network() {
        let t = ImageGen::new(None, None);
        let r = t
            .invoke(ToolCall {
                id: "1".into(),
                name: "image_gen".into(),
                args: json!({ "prompt": "a cat" }),
            })
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn tts_without_key_errors_without_network() {
        let t = Tts::new(None, None);
        let r = t
            .invoke(ToolCall {
                id: "1".into(),
                name: "tts".into(),
                args: json!({ "text": "hello" }),
            })
            .await;
        assert!(r.is_err());
    }
}
