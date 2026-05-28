//! In-memory [`MemoryStore`] for tests and bootstrap.
//!
//! Replace with a SQLite-backed implementation in V1.

use std::sync::Mutex;

use aonyx_core::{MemoryStore, Result};
use async_trait::async_trait;

/// A thread-safe in-memory store. Loses everything on drop.
#[derive(Default)]
pub struct InMemoryStore {
    diary: Mutex<Vec<(String, String)>>,
}

impl InMemoryStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MemoryStore for InMemoryStore {
    async fn diary_append(&self, project: &str, content: &str) -> Result<()> {
        self.diary
            .lock()
            .expect("diary mutex poisoned")
            .push((project.to_string(), content.to_string()));
        Ok(())
    }

    async fn hybrid_search(&self, _query: &str, _k: usize) -> Result<Vec<(String, f32)>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn diary_append_persists_for_session() {
        let store = InMemoryStore::new();
        store.diary_append("demo", "first entry").await.unwrap();
        assert_eq!(store.diary.lock().unwrap().len(), 1);
    }
}
