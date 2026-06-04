//! Text embeddings for semantic retrieval (RG1).
//!
//! [`Embedder`] is provider-agnostic. [`LocalEmbedder`] (feature `rag`) runs
//! fastembed-rs (ONNX, offline after a one-time model download) — see ADR-005
//! / ADR-009. A provider-backed embedder (OpenAI / Ollama) lands in Phase 1.

use aonyx_core::Result;
use async_trait::async_trait;

/// Produces dense vectors for text. Must be deterministic for a given
/// `(model, input)` so persisted vectors stay comparable across runs.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Stable model id (e.g. `"bge-m3"`), persisted beside each vector so a
    /// model change can be detected and the corpus re-indexed.
    fn model_id(&self) -> &str;

    /// Embedding dimensionality.
    fn dim(&self) -> usize;

    /// Embed a batch of texts → one vector per input, in order.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[cfg(feature = "rag")]
pub use local::LocalEmbedder;

#[cfg(feature = "rag")]
mod local {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use aonyx_core::{AonyxError, Result};
    use async_trait::async_trait;
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

    use super::Embedder;

    /// Local, offline embedder via fastembed-rs (ONNX). Quality-first default:
    /// **BAAI/bge-m3** (multilingual, dim 1024, no query/passage prefix
    /// needed). The model downloads once into `cache_dir`, then runs offline.
    pub struct LocalEmbedder {
        model: Arc<Mutex<TextEmbedding>>,
        model_id: String,
        dim: usize,
    }

    impl LocalEmbedder {
        /// Load the default quality model (bge-m3), downloading on first use
        /// into `cache_dir` (e.g. `~/.aonyx/models`).
        pub fn new(cache_dir: PathBuf) -> Result<Self> {
            Self::with_model(EmbeddingModel::BGEM3, "bge-m3", 1024, cache_dir)
        }

        /// Load a specific fastembed model. `dim` must match the model's output.
        pub fn with_model(
            model: EmbeddingModel,
            id: &str,
            dim: usize,
            cache_dir: PathBuf,
        ) -> Result<Self> {
            let _ = std::fs::create_dir_all(&cache_dir);
            let te = TextEmbedding::try_new(
                InitOptions::new(model)
                    .with_cache_dir(cache_dir)
                    .with_show_download_progress(true),
            )
            .map_err(|e| AonyxError::Memory(format!("load embedder '{id}': {e}")))?;
            Ok(Self {
                model: Arc::new(Mutex::new(te)),
                model_id: id.to_string(),
                dim,
            })
        }
    }

    #[async_trait]
    impl Embedder for LocalEmbedder {
        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn dim(&self) -> usize {
            self.dim
        }

        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            if texts.is_empty() {
                return Ok(Vec::new());
            }
            let model = Arc::clone(&self.model);
            let texts = texts.to_vec();
            // fastembed's embed() is blocking (ONNX inference) — keep it off
            // the async runtime.
            tokio::task::spawn_blocking(move || {
                let mut model = model
                    .lock()
                    .map_err(|_| AonyxError::Memory("embedder mutex poisoned".into()))?;
                model
                    .embed(texts, None)
                    .map_err(|e| AonyxError::Memory(format!("embed: {e}")))
            })
            .await
            .map_err(|e| AonyxError::Memory(format!("embed task join: {e}")))?
        }
    }
}
