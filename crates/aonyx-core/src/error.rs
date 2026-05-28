//! Canonical error type for Aonyx Agent.

use thiserror::Error;

/// Every fallible operation in the workspace returns `Result<T> = std::result::Result<T, AonyxError>`.
#[derive(Debug, Error)]
pub enum AonyxError {
    /// Configuration is missing or malformed.
    #[error("configuration error: {0}")]
    Config(String),

    /// LLM provider returned an error or could not be reached.
    #[error("provider error: {0}")]
    Provider(String),

    /// Memory palace operation failed (KG, diary, search, indexing).
    #[error("memory error: {0}")]
    Memory(String),

    /// Tool invocation failed before, during, or after execution.
    #[error("tool error: {0}")]
    Tool(String),

    /// Skill could not be loaded, matched, or executed.
    #[error("skill error: {0}")]
    Skill(String),

    /// MCP client or server protocol error.
    #[error("mcp error: {0}")]
    Mcp(String),

    /// A destructive action was rejected by the approval gate.
    #[error("approval rejected: {0}")]
    ApprovalRejected(String),

    /// Underlying filesystem / network I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// JSON (de)serialization failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Workspace-wide `Result` alias.
pub type Result<T> = std::result::Result<T, AonyxError>;
