//! Fallback chain router.
//!
//! Stores an ordered list of providers. `chat_stream` tries each one and
//! returns the first stream that starts successfully. Only the *opening*
//! handshake is retried; once a stream has started, subsequent errors
//! propagate up — we do not silently swap providers mid-response, which
//! would split a coherent answer across models.

use std::sync::Arc;

use aonyx_core::{AonyxError, ChatRequest, ChatStream, LlmProvider, Result};
use async_trait::async_trait;

/// An ordered list of fallback providers.
#[derive(Default, Clone)]
pub struct Router {
    providers: Vec<Arc<dyn LlmProvider>>,
}

impl Router {
    /// Build an empty router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a provider to the fallback chain.
    pub fn with(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// Snapshot the configured provider names — useful for logs and `aonyx config show`.
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    /// `true` when nothing has been registered.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[async_trait]
impl LlmProvider for Router {
    fn name(&self) -> &str {
        "router"
    }

    async fn chat_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        if self.providers.is_empty() {
            return Err(AonyxError::Provider(
                "router: no providers configured".into(),
            ));
        }
        let mut last_err: Option<AonyxError> = None;
        for p in &self.providers {
            match p.chat_stream(req.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    tracing::warn!(
                        provider = p.name(),
                        error = %e,
                        "router: provider failed, falling back"
                    );
                    last_err = Some(e);
                }
            }
        }
        Err(last_err
            .unwrap_or_else(|| AonyxError::Provider("router: every provider failed".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::{ChatChunk, ChatRequest};
    use async_trait::async_trait;

    struct AlwaysFails(&'static str);

    #[async_trait]
    impl LlmProvider for AlwaysFails {
        fn name(&self) -> &str {
            self.0
        }
        async fn chat_stream(&self, _req: ChatRequest) -> Result<ChatStream> {
            Err(AonyxError::Provider(format!("{} unavailable", self.0)))
        }
    }

    struct AlwaysWorks(&'static str);

    #[async_trait]
    impl LlmProvider for AlwaysWorks {
        fn name(&self) -> &str {
            self.0
        }
        async fn chat_stream(&self, _req: ChatRequest) -> Result<ChatStream> {
            let chunk = ChatChunk {
                delta_text: "ok".to_string(),
                tool_call: None,
                finished: true,
            };
            let stream = futures::stream::iter(vec![Ok::<_, AonyxError>(chunk)]);
            Ok(Box::pin(stream))
        }
    }

    fn req() -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages: Vec::new(),
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
        }
    }

    #[tokio::test]
    async fn empty_router_errors_out() {
        let r = Router::new();
        assert!(r.chat_stream(req()).await.is_err());
    }

    #[tokio::test]
    async fn first_provider_wins_when_it_succeeds() {
        let r = Router::new()
            .with(Arc::new(AlwaysWorks("a")))
            .with(Arc::new(AlwaysWorks("b")));
        assert!(r.chat_stream(req()).await.is_ok());
    }

    #[tokio::test]
    async fn falls_back_to_second_provider_when_first_fails() {
        let r = Router::new()
            .with(Arc::new(AlwaysFails("primary")))
            .with(Arc::new(AlwaysWorks("backup")));
        assert!(r.chat_stream(req()).await.is_ok());
    }

    #[tokio::test]
    async fn returns_last_error_when_every_provider_fails() {
        let r = Router::new()
            .with(Arc::new(AlwaysFails("primary")))
            .with(Arc::new(AlwaysFails("backup")));
        let err = r
            .chat_stream(req())
            .await
            .err()
            .expect("router should have errored");
        assert!(format!("{err}").contains("backup unavailable"));
    }

    #[tokio::test]
    async fn provider_names_lists_chain() {
        let r = Router::new()
            .with(Arc::new(AlwaysWorks("a")))
            .with(Arc::new(AlwaysFails("b")));
        assert_eq!(r.provider_names(), vec!["a", "b"]);
    }
}
