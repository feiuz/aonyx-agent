//! Claude Code provider — uses an installed `claude` binary as the backend.
//!
//! Lets users with a Claude subscription (Pro / Max / Team) or an
//! `ANTHROPIC_API_KEY` already wired into Claude Code drive Aonyx Agent
//! without configuring a second key in `~/.aonyx/config.toml`.
//!
//! ## How it works
//!
//! Each `chat_stream` call spawns:
//!
//! ```text
//! claude -p --output-format stream-json --verbose [--model <model>] [extra_args...]
//! ```
//!
//! The full conversation is written to the child's stdin as a single
//! plain-text transcript (system / user / assistant / tool messages, each
//! prefixed by a role tag). Claude Code emits one JSON object per line on
//! stdout. We forward `assistant` text content as [`ChatChunk`] deltas and
//! emit a terminal chunk on the `result` event.
//!
//! ## Behaviour notes
//!
//! - **Auth**: handled entirely by Claude Code. Aonyx never sees the user's
//!   credentials.
//! - **Streaming**: Claude Code may emit the full assistant message at every
//!   update (a partial-replace pattern) instead of pure deltas. We track the
//!   last surface text and forward the suffix when it grows, falling back to
//!   the full text otherwise.
//! - **Tool calls**: the V1 implementation forwards text only. Native Claude
//!   Code tool invocations (Read, Bash, …) happen inside the child process
//!   and never become Aonyx `ToolCall`s.
//! - **Prerequisites**: the `claude` binary must be installed and on `PATH`.
//!   A typical install: `npm install -g @anthropic-ai/claude-code` or
//!   download from <https://claude.ai/install>.

use std::process::Stdio;

use aonyx_core::{
    AonyxError, ChatChunk, ChatRequest, ChatStream, LlmProvider, Message, Result, Role,
};
use async_stream::try_stream;
use async_trait::async_trait;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Default binary name. Resolved via `PATH`.
pub const CLAUDE_DEFAULT_BIN: &str = "claude";

/// Claude Code provider.
#[derive(Clone)]
pub struct ClaudeCodeProvider {
    binary: String,
    extra_args: Vec<String>,
}

impl ClaudeCodeProvider {
    /// Build a provider that runs `claude` from `PATH`.
    pub fn new() -> Self {
        Self {
            binary: CLAUDE_DEFAULT_BIN.to_string(),
            extra_args: Vec::new(),
        }
    }

    /// Override the binary path (e.g. `"C:/Users/x/.claude/local/claude.exe"`).
    pub fn with_binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = binary.into();
        self
    }

    /// Append extra arguments forwarded to every `claude` invocation
    /// (e.g. `["--max-turns", "5"]`).
    pub fn with_extra_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    /// Inspect the configured binary path.
    pub fn binary(&self) -> &str {
        &self.binary
    }
}

impl Default for ClaudeCodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        "claude-code"
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let prompt = render_conversation(&req.messages);

        let mut cmd = Command::new(&self.binary);
        cmd.arg("-p")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");
        if !req.model.is_empty() {
            cmd.arg("--model").arg(&req.model);
        }
        for arg in &self.extra_args {
            cmd.arg(arg);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            AonyxError::Provider(format!(
                "claude-code spawn: {e}; is '{}' installed and on PATH?",
                self.binary
            ))
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| AonyxError::Provider(format!("claude-code stdin: {e}")))?;
            stdin
                .shutdown()
                .await
                .map_err(|e| AonyxError::Provider(format!("claude-code stdin close: {e}")))?;
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AonyxError::Provider("claude-code: no stdout pipe".into()))?;
        let mut reader = BufReader::new(stdout).lines();

        let chunk_stream = try_stream! {
            let mut last_text = String::new();
            let mut emitted_finish = false;
            loop {
                match reader.next_line().await {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Some(chunk) = parse_event_line(&line, &mut last_text) {
                            if chunk.finished {
                                emitted_finish = true;
                            }
                            yield chunk;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        Err(AonyxError::Provider(format!("claude-code read: {e}")))?;
                    }
                }
            }

            match child.wait().await {
                Ok(status) if !status.success() => {
                    Err(AonyxError::Provider(format!(
                        "claude-code exit {}",
                        status.code().unwrap_or(-1)
                    )))?;
                }
                Err(e) => {
                    Err(AonyxError::Provider(format!("claude-code wait: {e}")))?;
                }
                Ok(_) => {}
            }

            if !emitted_finish {
                yield ChatChunk {
                    delta_text: String::new(),
                    tool_call: None,
                    finished: true,
                };
            }
        };

        Ok(Box::pin(chunk_stream))
    }
}

fn render_conversation(messages: &[Message]) -> String {
    let mut out = String::new();
    for m in messages {
        let tag = match m.role {
            Role::System => "[system]",
            Role::User => "[user]",
            Role::Assistant => "[assistant]",
            Role::Tool => "[tool result]",
        };
        out.push_str(tag);
        out.push('\n');
        out.push_str(&m.content);
        out.push_str("\n\n");
    }
    out
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClaudeEvent {
    #[serde(rename = "assistant")]
    Assistant { message: ClaudeMessage },
    /// `result` marks end-of-turn; we only care about the tag, every payload
    /// field (subtype, result, cost_usd, duration_ms, …) is intentionally
    /// dropped via [`serde::de::IgnoredAny`].
    #[serde(rename = "result")]
    Result(serde::de::IgnoredAny),
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClaudeContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

fn extract_text(message: ClaudeMessage) -> String {
    let mut out = String::new();
    for c in message.content {
        if let ClaudeContent::Text { text } = c {
            out.push_str(&text);
        }
    }
    out
}

/// Parse one stream-json line, updating `last_text` for delta tracking.
pub(crate) fn parse_event_line(line: &str, last_text: &mut String) -> Option<ChatChunk> {
    let event: ClaudeEvent = serde_json::from_str(line).ok()?;
    match event {
        ClaudeEvent::Assistant { message } => {
            let full = extract_text(message);
            if full.is_empty() {
                return None;
            }
            // Partial-replace pattern: forward only the new suffix.
            if full.starts_with(last_text.as_str()) && full.len() > last_text.len() {
                let delta = full[last_text.len()..].to_string();
                *last_text = full;
                Some(ChatChunk {
                    delta_text: delta,
                    tool_call: None,
                    finished: false,
                })
            } else if full == *last_text {
                None
            } else {
                // Pure-delta stream: forward as-is and reset the surface.
                *last_text = full.clone();
                Some(ChatChunk {
                    delta_text: full,
                    tool_call: None,
                    finished: false,
                })
            }
        }
        ClaudeEvent::Result(_) => Some(ChatChunk {
            delta_text: String::new(),
            tool_call: None,
            finished: true,
        }),
        ClaudeEvent::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::Message;

    #[test]
    fn provider_name_is_claude_code() {
        let p = ClaudeCodeProvider::new();
        assert_eq!(p.name(), "claude-code");
        assert_eq!(p.binary(), CLAUDE_DEFAULT_BIN);
    }

    #[test]
    fn with_binary_overrides_default() {
        let p = ClaudeCodeProvider::new().with_binary("/opt/claude");
        assert_eq!(p.binary(), "/opt/claude");
    }

    #[test]
    fn render_conversation_tags_every_role() {
        let msgs = vec![
            Message::new(Role::System, "be brief"),
            Message::new(Role::User, "hi"),
            Message::new(Role::Assistant, "hello"),
            Message::new(Role::Tool, "tool said x"),
        ];
        let s = render_conversation(&msgs);
        assert!(s.contains("[system]"));
        assert!(s.contains("be brief"));
        assert!(s.contains("[user]"));
        assert!(s.contains("hi"));
        assert!(s.contains("[assistant]"));
        assert!(s.contains("hello"));
        assert!(s.contains("[tool result]"));
        assert!(s.contains("tool said x"));
    }

    #[test]
    fn parses_assistant_text_event() {
        let mut last = String::new();
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#;
        let got = parse_event_line(line, &mut last).expect("parsed");
        assert_eq!(got.delta_text, "Hello");
        assert!(!got.finished);
        assert_eq!(last, "Hello");
    }

    #[test]
    fn emits_delta_when_assistant_message_grows() {
        let mut last = String::from("Hello");
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}]}}"#;
        let got = parse_event_line(line, &mut last).expect("parsed");
        assert_eq!(got.delta_text, " world");
        assert_eq!(last, "Hello world");
    }

    #[test]
    fn duplicate_assistant_message_is_ignored() {
        let mut last = String::from("Hello");
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#;
        assert!(parse_event_line(line, &mut last).is_none());
    }

    #[test]
    fn replaced_assistant_message_emits_full_text() {
        let mut last = String::from("draft answer");
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"final reply"}]}}"#;
        let got = parse_event_line(line, &mut last).expect("parsed");
        assert_eq!(got.delta_text, "final reply");
        assert_eq!(last, "final reply");
    }

    #[test]
    fn result_event_marks_finished() {
        let mut last = String::new();
        let line = r#"{"type":"result","subtype":"success","result":"done","cost_usd":0.001,"duration_ms":1234,"num_turns":1,"session_id":"abc","is_error":false}"#;
        let got = parse_event_line(line, &mut last).expect("parsed");
        assert!(got.finished);
        assert!(got.delta_text.is_empty());
    }

    #[test]
    fn ignores_system_init_event() {
        let mut last = String::new();
        let line = r#"{"type":"system","subtype":"init","session_id":"abc"}"#;
        assert!(parse_event_line(line, &mut last).is_none());
    }

    #[test]
    fn ignores_non_text_content_blocks() {
        let mut last = String::new();
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"x","name":"Read","input":{}}]}}"#;
        assert!(parse_event_line(line, &mut last).is_none());
    }

    #[test]
    fn malformed_json_is_silently_skipped() {
        let mut last = String::new();
        assert!(parse_event_line("not json", &mut last).is_none());
    }
}
