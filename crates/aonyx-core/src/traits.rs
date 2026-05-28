//! Cross-cutting traits implemented by other crates.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{Message, Result, SafetyClass, ToolCall, ToolResult};

/// A minimal chat-completion request, provider-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Model identifier as understood by the provider.
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// JSON-schema-described tools available for this turn.
    pub tools: Vec<serde_json::Value>,
    /// Sampling temperature.
    pub temperature: Option<f32>,
}

/// A streamed delta from an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChunk {
    /// Incremental text content (may be empty for tool-call frames).
    pub delta_text: String,
    /// Tool call detected in this chunk, if any.
    pub tool_call: Option<ToolCall>,
    /// Set on the terminating chunk of the stream.
    pub finished: bool,
}

/// Abstract LLM provider (Anthropic, OpenAI, Ollama, …).
///
/// `#[async_trait]` keeps the trait object-safe so we can store providers
/// behind `Arc<dyn LlmProvider>` in the router.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable provider name (e.g. `"anthropic"`, `"ollama"`).
    fn name(&self) -> &str;

    /// Stream a chat completion. The default impl is intentionally absent —
    /// every provider must implement it.
    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<Box<dyn futures_stream_chunk::ChunkStream>>;
}

/// Re-export a thin type alias so trait objects don't expose `futures::Stream` directly.
pub mod futures_stream_chunk {
    //! Indirection placeholder; replaced by a real `Stream<Item = Result<ChatChunk>>`
    //! when `aonyx-llm` ships its first real provider.
    use super::ChatChunk;

    /// Sealed marker trait for chunk streams.
    pub trait ChunkStream: Send {
        /// Pull the next chunk (blocking placeholder; will become async-poll-based).
        fn next_chunk(&mut self) -> Option<crate::Result<ChatChunk>>;
    }
}

/// Memory palace store — implemented by `aonyx-memory`.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Append a free-form note to the project diary.
    async fn diary_append(&self, project: &str, content: &str) -> Result<()>;

    /// Hybrid search across chunks (BM25 + vectors + RRF). Returns top-k chunk ids + scores.
    async fn hybrid_search(&self, query: &str, k: usize) -> Result<Vec<(String, f32)>>;
}

/// A registered tool — implemented by every module in `aonyx-tools`.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Stable tool name as referenced by the LLM.
    fn name(&self) -> &str;

    /// JSON-schema describing valid `args` for this tool.
    fn schema(&self) -> serde_json::Value;

    /// Safety class — see [`SafetyClass`].
    fn classify(&self) -> SafetyClass;

    /// Execute the tool against validated arguments.
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult>;
}

/// A loader of skills (markdown + YAML frontmatter), implemented by `aonyx-skills`.
#[async_trait]
pub trait SkillSource: Send + Sync {
    /// List skill identifiers currently visible to this source.
    async fn list(&self) -> Result<Vec<String>>;

    /// Activate or refresh skills whose triggers match a context.
    async fn match_active(&self, query: &str, project: Option<&str>) -> Result<Vec<String>>;
}
