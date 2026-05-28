//! # aonyx-llm
//!
//! Multi-provider LLM router. One [`LlmProvider`](aonyx_core::LlmProvider) trait,
//! several implementations, one configurable fallback chain.
//!
//! ## V1 providers
//! - [`anthropic`] — native Anthropic API.
//! - [`openai`] — OpenAI Chat Completions.
//! - [`openrouter`] — OpenAI-compatible with model routing.
//! - [`ollama`] — local Ollama `/api/chat`.
//! - [`lm_studio`] — OpenAI-compatible custom base URL.
//! - [`nous_portal`] — Nous Portal endpoint.
//!
//! ## Router
//! A [`Router`] holds an ordered list of providers and forwards each request to
//! the first one that does not error; rate-limit and 5xx responses fail over.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod anthropic;
pub mod lm_studio;
pub mod nous_portal;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod router;

pub use router::Router;
