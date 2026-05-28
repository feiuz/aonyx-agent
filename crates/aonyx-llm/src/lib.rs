//! # aonyx-llm
//!
//! Multi-provider LLM router. One [`LlmProvider`](aonyx_core::LlmProvider) trait,
//! several implementations, one configurable fallback chain.
//!
//! ## V1 + P2.1 providers
//! - [`anthropic`] — native Anthropic Messages API (streaming SSE).
//! - [`openai_compat`] — shared OpenAI-compatible backend.
//!   - [`openai`] — public OpenAI API (`https://api.openai.com`).
//!   - [`openrouter`] — OpenRouter aggregator, with optional attribution headers.
//!   - [`lm_studio`] — local OpenAI-compatible LM Studio server.
//! - [`ollama`] — local Ollama (`/api/chat`), JSON-lines streaming.
//! - [`nous_portal`] — Nous Portal endpoint (deferred to P3).
//!
//! ## Router
//! [`Router`] holds an ordered list of providers and forwards each request to
//! the first one whose stream opens successfully.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod anthropic;
pub mod lm_studio;
pub mod nous_portal;
pub mod ollama;
pub mod openai;
pub mod openai_compat;
pub mod openrouter;
pub mod router;

pub use ollama::{OllamaProvider, OLLAMA_DEFAULT_BASE_URL};
pub use openai_compat::OpenAiCompatProvider;
pub use router::Router;
