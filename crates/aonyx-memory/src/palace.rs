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
use std::sync::Arc;

use aonyx_core::{AonyxError, MemoryStore, Result};
use async_trait::async_trait;

use crate::chunks::{Chunk, ChunkId, ChunksStore, ScoredChunk, SqliteChunksStore};
use crate::diary::{DiaryEntry, DiaryStore, SqliteDiaryStore};
use crate::embed::Embedder;
use crate::kg::SqliteKgStore;

/// The composed memory palace.
#[derive(Clone)]
pub struct Palace {
    /// Knowledge-graph store.
    pub kg: SqliteKgStore,
    /// Narrative diary store.
    pub diary: SqliteDiaryStore,
    /// Searchable chunks store (BM25 via FTS5).
    pub chunks: SqliteChunksStore,
    /// Optional embedder. When set, [`MemoryStore::hybrid_search`] fuses BM25
    /// with vector search (RRF); when `None`, search is BM25-only (default).
    embedder: Option<Arc<dyn Embedder>>,
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
        let chunks = SqliteChunksStore::open(dir.join("chunks.db"))?;
        Ok(Self {
            kg,
            diary,
            chunks,
            embedder: None,
        })
    }

    /// Open an entirely in-memory palace — for tests.
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            kg: SqliteKgStore::open_in_memory()?,
            diary: SqliteDiaryStore::open_in_memory()?,
            chunks: SqliteChunksStore::open_in_memory()?,
            embedder: None,
        })
    }

    /// Default palace directory layout for the standard CLI: `./.aonyx/`.
    pub fn default_project_dir(project_root: impl AsRef<Path>) -> PathBuf {
        project_root.as_ref().join(".aonyx")
    }

    /// Attach an embedder so [`MemoryStore::hybrid_search`] runs hybrid
    /// (BM25 + vectors via RRF) instead of BM25-only.
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// Ingest free text under `project` / `source`: split it, append the chunks
    /// (BM25), and — when an embedder is configured — embed and store the vectors
    /// so the document joins hybrid search. Returns the number of chunks written.
    pub async fn ingest_text(&self, project: &str, source: &str, text: &str) -> Result<usize> {
        let parts = split_text(text);
        if parts.is_empty() {
            return Ok(0);
        }
        let mut ids = Vec::with_capacity(parts.len());
        for p in &parts {
            ids.push(
                self.chunks
                    .append(Chunk::new(project, source, p.as_str()).with_kind("doc"))
                    .await?,
            );
        }
        if let Some(emb) = &self.embedder {
            let vecs = emb.embed(&parts).await?;
            for (id, v) in ids.iter().zip(vecs) {
                self.chunks.upsert_vector(*id, emb.model_id(), &v).await?;
            }
        }
        Ok(parts.len())
    }
}

/// Split text into retrieval-sized chunks: paragraph boundaries (blank lines),
/// merged up to a target length so each chunk is a coherent unit.
fn split_text(text: &str) -> Vec<String> {
    const TARGET: usize = 1500;
    let mut out = Vec::new();
    let mut cur = String::new();
    for para in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if !cur.is_empty() && cur.len() + para.len() + 2 > TARGET {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(para);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[async_trait]
impl MemoryStore for Palace {
    async fn diary_append(&self, project: &str, content: &str) -> Result<()> {
        self.diary.append(DiaryEntry::new(project, content)).await?;
        Ok(())
    }

    async fn hybrid_search(&self, query: &str, k: usize) -> Result<Vec<(String, f32)>> {
        Ok(self
            .search(query, k)
            .await?
            .into_iter()
            .map(|sc| (sc.chunk.content, sc.score))
            .collect())
    }
}

impl Palace {
    /// Ranked retrieval returning **full chunks** (with `project` / `source`
    /// for citations). Hybrid (BM25 + vectors via RRF k=60) when an embedder is
    /// configured; BM25-only otherwise (default + offline fallback). This is
    /// what the built-in `rag_search` tool surfaces.
    pub async fn search(&self, query: &str, k: usize) -> Result<Vec<ScoredChunk>> {
        // Pull more candidates than `k` from each arm so RRF has material.
        let cand = (k * 4).max(20);
        let bm25 = self.chunks.search_bm25(None, query, cand).await?;

        let Some(embedder) = &self.embedder else {
            let mut bm25 = bm25;
            bm25.truncate(k);
            return Ok(bm25);
        };
        let Some(qv) = embedder
            .embed(&[query.to_string()])
            .await?
            .into_iter()
            .next()
        else {
            let mut bm25 = bm25;
            bm25.truncate(k);
            return Ok(bm25);
        };
        let vectors = self.chunks.vector_search(None, &qv, cand).await?;
        Ok(rrf_fuse(&[bm25, vectors], k))
    }
}

/// Reciprocal Rank Fusion (k=60) over several ranked chunk lists, dedup by
/// chunk id, top-`limit`. Each chunk's score becomes its fused RRF score.
fn rrf_fuse(lists: &[Vec<ScoredChunk>], limit: usize) -> Vec<ScoredChunk> {
    use std::collections::HashMap;
    const RRF_K: f32 = 60.0;
    let mut acc: HashMap<ChunkId, (f32, Chunk)> = HashMap::new();
    for list in lists {
        for (rank, sc) in list.iter().enumerate() {
            let contrib = 1.0 / (RRF_K + rank as f32 + 1.0);
            acc.entry(sc.chunk.id)
                .or_insert_with(|| (0.0, sc.chunk.clone()))
                .0 += contrib;
        }
    }
    let mut fused: Vec<ScoredChunk> = acc
        .into_values()
        .map(|(score, chunk)| ScoredChunk { chunk, score })
        .collect();
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fused.truncate(limit);
    fused
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
        assert_eq!(palace.chunks.count(None).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn hybrid_search_finds_bm25_matches() {
        use crate::chunks::{Chunk, ChunksStore};
        let palace = Palace::open_in_memory().unwrap();
        palace
            .chunks
            .append(Chunk::new(
                "demo",
                "src/runner.rs",
                "the agent runner loops until no tool call remains",
            ))
            .await
            .unwrap();
        let hits = palace.hybrid_search("agent runner", 5).await.unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].0.contains("runner"));
        assert!(hits[0].1 > 0.0);
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
        assert!(palace_dir.join("chunks.db").exists());
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
    async fn hybrid_search_empty_store_is_empty() {
        let palace = Palace::open_in_memory().unwrap();
        let results = palace.hybrid_search("anything", 5).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn hybrid_search_fuses_bm25_and_vectors() {
        use crate::chunks::{Chunk, ChunksStore};

        struct FakeEmbedder;
        #[async_trait]
        impl Embedder for FakeEmbedder {
            fn model_id(&self) -> &str {
                "fake"
            }
            fn dim(&self) -> usize {
                3
            }
            async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
                Ok(texts
                    .iter()
                    .map(|t| {
                        if t.contains("agent") {
                            vec![1.0, 0.0, 0.0]
                        } else if t.contains("memory") {
                            vec![0.0, 1.0, 0.0]
                        } else {
                            vec![0.0, 0.0, 1.0]
                        }
                    })
                    .collect())
            }
        }

        let palace = Palace::open_in_memory()
            .unwrap()
            .with_embedder(Arc::new(FakeEmbedder));
        let a = Chunk::new("demo", "a.rs", "the agent loop runs tools");
        let aid = a.id;
        palace.chunks.append(a).await.unwrap();
        palace
            .chunks
            .upsert_vector(aid, "fake", &[1.0, 0.0, 0.0])
            .await
            .unwrap();
        let b = Chunk::new("demo", "b.rs", "memory palace notes");
        let bid = b.id;
        palace.chunks.append(b).await.unwrap();
        palace
            .chunks
            .upsert_vector(bid, "fake", &[0.0, 1.0, 0.0])
            .await
            .unwrap();

        // "agent" → query vector [1,0,0] closest to A; BM25 also hits A.
        let hits = palace.hybrid_search("agent", 5).await.unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].0.contains("agent"));
    }
}
