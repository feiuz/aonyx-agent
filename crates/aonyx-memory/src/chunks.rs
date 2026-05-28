//! Searchable chunks store backed by SQLite FTS5.
//!
//! Port reference: Aonyx RAG `rag_system/utils/bm25_store.py` + `utils/hybrid_search.py`.
//!
//! ## V1 scope (this file)
//! - Chunk = a piece of text + project + source + timestamp + free-form metadata.
//! - SQLite **FTS5** virtual table provides BM25-ranked full-text search out
//!   of the box, with a `unicode61 remove_diacritics 2` tokenizer that survives
//!   accents.
//! - `search_bm25(project?, query, k)` returns the top-`k` chunks ordered by
//!   relevance, with positive `score = -bm25(...)` so larger = better.
//!
//! ## V1.1 (deferred)
//! - Local embeddings via `fastembed-rs` (ONNX, ~30 MB model).
//! - HNSW index for vector ANN search.
//! - **RRF** fusion with `k = 60` combining BM25 + vectors.
//! - Exponential temporal boost on recent chunks.
//!
//! The trait signature already accepts a `mode` field so V1.1 can extend it
//! without breaking callers.

use std::path::Path;
use std::sync::{Arc, Mutex};

use aonyx_core::{AonyxError, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Stable identifier for a [`Chunk`].
pub type ChunkId = Uuid;

/// A piece of indexable text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    /// Stable id (UUID v4 by default).
    pub id: ChunkId,
    /// Project slug this chunk belongs to.
    pub project: String,
    /// Source identifier (path, url, doc id).
    pub source: String,
    /// Raw chunk text.
    pub content: String,
    /// Creation timestamp.
    pub ts: DateTime<Utc>,
    /// Optional classifier (`"code"`, `"note"`, `"diary"`, `"doc"`).
    pub kind: Option<String>,
    /// Free-form JSON metadata (e.g. AST symbol name + line range for code chunks).
    #[serde(default)]
    pub metadata: JsonValue,
}

impl Chunk {
    /// Build a new chunk with sensible defaults.
    pub fn new(
        project: impl Into<String>,
        source: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            project: project.into(),
            source: source.into(),
            content: content.into(),
            ts: Utc::now(),
            kind: None,
            metadata: JsonValue::Null,
        }
    }

    /// Attach a classifier.
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Attach JSON metadata.
    pub fn with_metadata(mut self, metadata: JsonValue) -> Self {
        self.metadata = metadata;
        self
    }
}

/// A search hit: a chunk and its score (larger = more relevant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredChunk {
    /// The matched chunk.
    pub chunk: Chunk,
    /// Relevance score (positive; we flip SQLite's negative BM25).
    pub score: f32,
}

/// Async chunks store.
#[async_trait]
pub trait ChunksStore: Send + Sync {
    /// Append a new chunk.
    async fn append(&self, chunk: Chunk) -> Result<ChunkId>;

    /// BM25 search.
    ///
    /// `project = None` searches across every project; `Some(p)` scopes to one.
    /// `k` caps the number of hits.
    async fn search_bm25(
        &self,
        project: Option<&str>,
        query: &str,
        k: usize,
    ) -> Result<Vec<ScoredChunk>>;

    /// Total chunk count, optionally scoped to a project.
    async fn count(&self, project: Option<&str>) -> Result<usize>;
}

/// SQLite-backed [`ChunksStore`] using FTS5 for BM25 ranking.
#[derive(Clone)]
pub struct SqliteChunksStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteChunksStore {
    /// Open (or create) the chunks database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .map_err(|e| AonyxError::Memory(format!("open chunks db: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database — convenient for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AonyxError::Memory(format!("open in-memory chunks: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(MIGRATION_V1)
            .map_err(|e| AonyxError::Memory(format!("migrate chunks schema: {e}")))?;
        Ok(())
    }
}

const MIGRATION_V1: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    uuid           UNINDEXED,
    project        UNINDEXED,
    source         UNINDEXED,
    ts             UNINDEXED,
    kind           UNINDEXED,
    metadata_json  UNINDEXED,
    content,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#;

#[async_trait]
impl ChunksStore for SqliteChunksStore {
    async fn append(&self, chunk: Chunk) -> Result<ChunkId> {
        let conn = self.conn.clone();
        let id = chunk.id;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let lock = conn.lock().expect("chunks mutex poisoned");
            lock.execute(
                r#"
                INSERT INTO chunks_fts (uuid, project, source, ts, kind, metadata_json, content)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    chunk.id.to_string(),
                    chunk.project,
                    chunk.source,
                    chunk.ts.to_rfc3339(),
                    chunk.kind,
                    serde_json::to_string(&chunk.metadata).ok(),
                    chunk.content,
                ],
            )
            .map_err(|e| AonyxError::Memory(format!("chunks append: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("chunks append join: {e}")))??;
        Ok(id)
    }

    async fn search_bm25(
        &self,
        project: Option<&str>,
        query: &str,
        k: usize,
    ) -> Result<Vec<ScoredChunk>> {
        let conn = self.conn.clone();
        let query = query.to_string();
        let project = project.map(str::to_string);
        let limit = k as i64;
        tokio::task::spawn_blocking(move || -> Result<Vec<ScoredChunk>> {
            let lock = conn.lock().expect("chunks mutex poisoned");
            let (sql, with_project) = if project.is_some() {
                (
                    "SELECT uuid, project, source, ts, kind, metadata_json, content, bm25(chunks_fts) AS score
                     FROM chunks_fts
                     WHERE chunks_fts MATCH ?1 AND project = ?2
                     ORDER BY score ASC
                     LIMIT ?3",
                    true,
                )
            } else {
                (
                    "SELECT uuid, project, source, ts, kind, metadata_json, content, bm25(chunks_fts) AS score
                     FROM chunks_fts
                     WHERE chunks_fts MATCH ?1
                     ORDER BY score ASC
                     LIMIT ?2",
                    false,
                )
            };
            let mut stmt = lock
                .prepare(sql)
                .map_err(|e| AonyxError::Memory(format!("prepare search_bm25: {e}")))?;
            let row_iter = if with_project {
                stmt.query_map(
                    params![query, project.as_ref().expect("project guarded above"), limit],
                    decode_row,
                )
            } else {
                stmt.query_map(params![query, limit], decode_row)
            }
            .map_err(|e| AonyxError::Memory(format!("query search_bm25: {e}")))?;

            let mut out = Vec::new();
            for r in row_iter {
                out.push(r.map_err(|e| AonyxError::Memory(format!("row decode: {e}")))?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("chunks search join: {e}")))?
    }

    async fn count(&self, project: Option<&str>) -> Result<usize> {
        let conn = self.conn.clone();
        let project = project.map(str::to_string);
        tokio::task::spawn_blocking(move || -> Result<usize> {
            let lock = conn.lock().expect("chunks mutex poisoned");
            let n: i64 = match project {
                Some(p) => lock
                    .query_row(
                        "SELECT COUNT(*) FROM chunks_fts WHERE project = ?1",
                        params![p],
                        |r| r.get(0),
                    )
                    .map_err(|e| AonyxError::Memory(format!("count: {e}")))?,
                None => lock
                    .query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
                    .map_err(|e| AonyxError::Memory(format!("count: {e}")))?,
            };
            Ok(n.max(0) as usize)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("chunks count join: {e}")))?
    }
}

fn decode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScoredChunk> {
    let uuid_str: String = row.get(0)?;
    let project: String = row.get(1)?;
    let source: String = row.get(2)?;
    let ts_raw: String = row.get(3)?;
    let kind: Option<String> = row.get(4)?;
    let metadata_raw: Option<String> = row.get(5)?;
    let content: String = row.get(6)?;
    let raw_score: f64 = row.get(7)?;

    let id = Uuid::parse_str(&uuid_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let ts = DateTime::parse_from_rfc3339(&ts_raw)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let metadata = metadata_raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(JsonValue::Null);

    Ok(ScoredChunk {
        chunk: Chunk {
            id,
            project,
            source,
            content,
            ts,
            kind,
            metadata,
        },
        // SQLite's bm25() returns negative values; flip the sign so larger = better.
        score: -(raw_score as f32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_store() -> SqliteChunksStore {
        let store = SqliteChunksStore::open_in_memory().unwrap();
        store
            .append(Chunk::new(
                "demo",
                "src/lib.rs",
                "the agent loops over tool calls",
            ))
            .await
            .unwrap();
        store
            .append(Chunk::new(
                "demo",
                "src/runner.rs",
                "compaction kicks in at fifty percent",
            ))
            .await
            .unwrap();
        store
            .append(Chunk::new("other", "README.md", "another project entirely"))
            .await
            .unwrap();
        store
    }

    #[tokio::test]
    async fn append_then_count() {
        let store = SqliteChunksStore::open_in_memory().unwrap();
        store
            .append(Chunk::new("demo", "a.txt", "hello aonyx"))
            .await
            .unwrap();
        assert_eq!(store.count(None).await.unwrap(), 1);
        assert_eq!(store.count(Some("demo")).await.unwrap(), 1);
        assert_eq!(store.count(Some("other")).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn search_bm25_returns_relevant_chunks() {
        let store = seeded_store().await;
        let hits = store.search_bm25(None, "compaction", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].chunk.content.contains("compaction"));
        assert!(hits[0].score > 0.0);
    }

    #[tokio::test]
    async fn search_bm25_can_scope_to_project() {
        let store = seeded_store().await;
        let in_demo = store
            .search_bm25(Some("demo"), "project OR agent", 10)
            .await
            .unwrap();
        let in_other = store
            .search_bm25(Some("other"), "project OR agent", 10)
            .await
            .unwrap();
        assert!(in_demo.iter().all(|h| h.chunk.project == "demo"));
        assert!(in_other.iter().all(|h| h.chunk.project == "other"));
    }

    #[tokio::test]
    async fn search_bm25_returns_empty_when_no_match() {
        let store = seeded_store().await;
        let hits = store
            .search_bm25(None, "nothing_should_match_this", 10)
            .await
            .unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn search_bm25_honours_limit() {
        let store = SqliteChunksStore::open_in_memory().unwrap();
        for i in 0..5 {
            store
                .append(Chunk::new("demo", "x", format!("repeat token {i}")))
                .await
                .unwrap();
        }
        let hits = store.search_bm25(None, "repeat", 2).await.unwrap();
        assert_eq!(hits.len(), 2);
    }
}
