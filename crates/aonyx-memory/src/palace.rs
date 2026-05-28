//! Unified memory palace — composes [`SqliteKgStore`] + [`SqliteDiaryStore`]
//! behind a single [`MemoryStore`] facade.
//!
//! Storage layout on disk:
//! ```text
//! <palace_dir>/
//! ├── kg.db        # entities + relations
//! └── diary.db     # narrative log
//! ```
//!
//! V1 keeps the two backends in separate SQLite files so each can be opened,
//! exported, or repaired independently. A future migration may consolidate
//! them into a single file once we add chunks + cross-links.

use std::path::{Path, PathBuf};

use aonyx_core::{AonyxError, MemoryStore, Result};
use async_trait::async_trait;

use crate::diary::{DiaryEntry, DiaryStore, SqliteDiaryStore};
use crate::kg::SqliteKgStore;

/// The composed memory palace.
#[derive(Clone)]
pub struct Palace {
    /// Knowledge-graph store.
    pub kg: SqliteKgStore,
    /// Narrative diary store.
    pub diary: SqliteDiaryStore,
}

impl Palace {
    /// Open (or create) a palace under `dir`.
    ///
    /// `dir` will be created if it does not yet exist.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)
            .map_err(|e| AonyxError::Memory(format!("create palace dir {dir:?}: {e}")))?;
        let kg = SqliteKgStore::open(dir.join("kg.db"))?;
        let diary = SqliteDiaryStore::open(dir.join("diary.db"))?;
        Ok(Self { kg, diary })
    }

    /// Open an entirely in-memory palace — for tests.
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            kg: SqliteKgStore::open_in_memory()?,
            diary: SqliteDiaryStore::open_in_memory()?,
        })
    }

    /// Default palace directory layout for the standard CLI: `./.aonyx/`.
    pub fn default_project_dir(project_root: impl AsRef<Path>) -> PathBuf {
        project_root.as_ref().join(".aonyx")
    }
}

#[async_trait]
impl MemoryStore for Palace {
    async fn diary_append(&self, project: &str, content: &str) -> Result<()> {
        self.diary.append(DiaryEntry::new(project, content)).await?;
        Ok(())
    }

    async fn hybrid_search(&self, _query: &str, _k: usize) -> Result<Vec<(String, f32)>> {
        // V2 will compose BM25 (FTS5) + vector search + RRF.
        // V1 returns an empty result set so the agent loop can call it safely.
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kg::{Entity, KgStore};

    #[tokio::test]
    async fn open_in_memory_starts_empty() {
        let palace = Palace::open_in_memory().unwrap();
        assert_eq!(palace.kg.count_entities().await.unwrap(), 0);
        assert_eq!(palace.diary.count("demo").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn memory_store_diary_append_persists() {
        let palace = Palace::open_in_memory().unwrap();
        palace
            .diary_append("demo", "first note from the runner")
            .await
            .unwrap();
        let entries = palace.diary.all("demo").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "first note from the runner");
    }

    #[tokio::test]
    async fn kg_and_diary_are_independent() {
        let palace = Palace::open_in_memory().unwrap();
        palace
            .kg
            .upsert_entity(Entity::new("Damien", "person"))
            .await
            .unwrap();
        palace.diary_append("demo", "noted").await.unwrap();
        assert_eq!(palace.kg.count_entities().await.unwrap(), 1);
        assert_eq!(palace.diary.count("demo").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn open_creates_directory_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let palace_dir = tmp.path().join("palace");
        let palace = Palace::open(&palace_dir).unwrap();
        assert!(palace_dir.join("kg.db").exists());
        assert!(palace_dir.join("diary.db").exists());
        palace
            .diary_append("demo", "persistent note")
            .await
            .unwrap();
        drop(palace);

        // Reopen and confirm the note is still there.
        let palace = Palace::open(&palace_dir).unwrap();
        assert_eq!(palace.diary.count("demo").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn hybrid_search_is_a_stub_in_v1() {
        let palace = Palace::open_in_memory().unwrap();
        let results = palace.hybrid_search("anything", 5).await.unwrap();
        assert!(results.is_empty());
    }
}
