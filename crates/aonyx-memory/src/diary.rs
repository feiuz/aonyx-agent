//! Append-only narrative log per project.
//!
//! Port reference: Aonyx RAG `rag_system/agent/diary.py`.
//!
//! The diary is the agent's running journal: short, dated, free-form notes
//! about what happened, what was decided, and what surprised the agent. Unlike
//! the [`crate::kg::KgStore`] — which models *structured* facts — the diary
//! stores prose. The two complement each other: a diary entry can reference
//! a KG entity, and a KG fact can cite a diary entry as its source.
//!
//! ## Schema (idempotent SQLite migration)
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS diary (
//!     id        TEXT PRIMARY KEY,
//!     project   TEXT NOT NULL,
//!     ts        TEXT NOT NULL,
//!     content   TEXT NOT NULL,
//!     kind      TEXT,
//!     refs_json TEXT
//! );
//! CREATE INDEX IF NOT EXISTS idx_diary_project_ts ON diary(project, ts DESC);
//! ```

use std::path::Path;
use std::sync::{Arc, Mutex};

use aonyx_core::{AonyxError, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Stable identifier for a [`DiaryEntry`].
pub type DiaryEntryId = Uuid;

/// A single diary entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiaryEntry {
    /// Stable id (UUID v4 by default).
    pub id: DiaryEntryId,
    /// Project slug this entry belongs to.
    pub project: String,
    /// Wall-clock timestamp the entry was written.
    pub ts: DateTime<Utc>,
    /// Free-form markdown body.
    pub content: String,
    /// Optional classifier (`"decision"`, `"fact"`, `"note"`, `"surprise"`).
    pub kind: Option<String>,
    /// Structured cross-references (KG entity ids, doc ids, urls).
    #[serde(default)]
    pub refs: JsonValue,
}

impl DiaryEntry {
    /// Build a new free-form entry stamped with the current time.
    pub fn new(project: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            project: project.into(),
            ts: Utc::now(),
            content: content.into(),
            kind: None,
            refs: JsonValue::Null,
        }
    }

    /// Attach a classifier (`"decision"`, `"fact"`, …).
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Attach a JSON references payload.
    pub fn with_refs(mut self, refs: JsonValue) -> Self {
        self.refs = refs;
        self
    }
}

/// Asynchronous diary store.
#[async_trait]
pub trait DiaryStore: Send + Sync {
    /// Append a new entry.
    async fn append(&self, entry: DiaryEntry) -> Result<DiaryEntryId>;

    /// List the most recent entries for a project (newest first).
    async fn recent(&self, project: &str, limit: usize) -> Result<Vec<DiaryEntry>>;

    /// List every entry for a project (newest first).
    async fn all(&self, project: &str) -> Result<Vec<DiaryEntry>>;

    /// Count entries for a project.
    async fn count(&self, project: &str) -> Result<usize>;
}

/// SQLite-backed [`DiaryStore`].
#[derive(Clone)]
pub struct SqliteDiaryStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteDiaryStore {
    /// Open (or create) the diary database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .map_err(|e| AonyxError::Memory(format!("open diary db: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database — convenient for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AonyxError::Memory(format!("open in-memory diary: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(MIGRATION_V1)
            .map_err(|e| AonyxError::Memory(format!("migrate diary schema: {e}")))?;
        Ok(())
    }
}

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS diary (
    id        TEXT PRIMARY KEY,
    project   TEXT NOT NULL,
    ts        TEXT NOT NULL,
    content   TEXT NOT NULL,
    kind      TEXT,
    refs_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_diary_project_ts ON diary(project, ts DESC);
"#;

const DIARY_COLUMNS: &str = "id, project, ts, content, kind, refs_json";

fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<DiaryEntry> {
    let id_str: String = row.get(0)?;
    let project: String = row.get(1)?;
    let ts_raw: String = row.get(2)?;
    let content: String = row.get(3)?;
    let kind: Option<String> = row.get(4)?;
    let refs_json: Option<String> = row.get(5)?;

    let id = Uuid::parse_str(&id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let ts = DateTime::parse_from_rfc3339(&ts_raw)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let refs = refs_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(JsonValue::Null);

    Ok(DiaryEntry {
        id,
        project,
        ts,
        content,
        kind,
        refs,
    })
}

#[async_trait]
impl DiaryStore for SqliteDiaryStore {
    async fn append(&self, entry: DiaryEntry) -> Result<DiaryEntryId> {
        let conn = self.conn.clone();
        let id = entry.id;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let lock = conn.lock().expect("diary mutex poisoned");
            lock.execute(
                r#"
                INSERT INTO diary (id, project, ts, content, kind, refs_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    entry.id.to_string(),
                    entry.project,
                    entry.ts.to_rfc3339(),
                    entry.content,
                    entry.kind,
                    serde_json::to_string(&entry.refs).ok(),
                ],
            )
            .map_err(|e| AonyxError::Memory(format!("diary append: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("diary append join: {e}")))??;
        Ok(id)
    }

    async fn recent(&self, project: &str, limit: usize) -> Result<Vec<DiaryEntry>> {
        let conn = self.conn.clone();
        let project = project.to_string();
        let limit = limit as i64;
        tokio::task::spawn_blocking(move || -> Result<Vec<DiaryEntry>> {
            let lock = conn.lock().expect("diary mutex poisoned");
            let sql = format!(
                "SELECT {DIARY_COLUMNS} FROM diary WHERE project = ?1 ORDER BY ts DESC LIMIT ?2"
            );
            let mut stmt = lock
                .prepare(&sql)
                .map_err(|e| AonyxError::Memory(format!("prepare diary recent: {e}")))?;
            let rows = stmt
                .query_map(params![project, limit], entry_from_row)
                .map_err(|e| AonyxError::Memory(format!("query diary recent: {e}")))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(|e| AonyxError::Memory(format!("row decode: {e}")))?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("diary recent join: {e}")))?
    }

    async fn all(&self, project: &str) -> Result<Vec<DiaryEntry>> {
        let conn = self.conn.clone();
        let project = project.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<DiaryEntry>> {
            let lock = conn.lock().expect("diary mutex poisoned");
            let sql =
                format!("SELECT {DIARY_COLUMNS} FROM diary WHERE project = ?1 ORDER BY ts DESC");
            let mut stmt = lock
                .prepare(&sql)
                .map_err(|e| AonyxError::Memory(format!("prepare diary all: {e}")))?;
            let rows = stmt
                .query_map(params![project], entry_from_row)
                .map_err(|e| AonyxError::Memory(format!("query diary all: {e}")))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(|e| AonyxError::Memory(format!("row decode: {e}")))?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("diary all join: {e}")))?
    }

    async fn count(&self, project: &str) -> Result<usize> {
        let conn = self.conn.clone();
        let project = project.to_string();
        tokio::task::spawn_blocking(move || -> Result<usize> {
            let lock = conn.lock().expect("diary mutex poisoned");
            let n: i64 = lock
                .query_row(
                    "SELECT COUNT(*) FROM diary WHERE project = ?1",
                    params![project],
                    |r| r.get(0),
                )
                .map_err(|e| AonyxError::Memory(format!("diary count: {e}")))?;
            Ok(n.max(0) as usize)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("diary count join: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn append_then_count() {
        let store = SqliteDiaryStore::open_in_memory().unwrap();
        store
            .append(DiaryEntry::new("demo", "first note"))
            .await
            .unwrap();
        assert_eq!(store.count("demo").await.unwrap(), 1);
        assert_eq!(store.count("other").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn recent_returns_newest_first() {
        let store = SqliteDiaryStore::open_in_memory().unwrap();
        store
            .append(DiaryEntry::new("demo", "older"))
            .await
            .unwrap();
        // RFC 3339 has millisecond resolution; sleep a tick so timestamps differ.
        tokio::time::sleep(Duration::from_millis(5)).await;
        store
            .append(DiaryEntry::new("demo", "newer"))
            .await
            .unwrap();

        let recent = store.recent("demo", 10).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].content, "newer");
        assert_eq!(recent[1].content, "older");
    }

    #[tokio::test]
    async fn recent_honours_limit() {
        let store = SqliteDiaryStore::open_in_memory().unwrap();
        for i in 0..5 {
            store
                .append(DiaryEntry::new("demo", format!("note {i}")))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let recent = store.recent("demo", 2).await.unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[tokio::test]
    async fn entries_are_project_scoped() {
        let store = SqliteDiaryStore::open_in_memory().unwrap();
        store
            .append(DiaryEntry::new("a", "only-in-a"))
            .await
            .unwrap();
        store
            .append(DiaryEntry::new("b", "only-in-b"))
            .await
            .unwrap();

        let in_a = store.all("a").await.unwrap();
        let in_b = store.all("b").await.unwrap();
        assert_eq!(in_a.len(), 1);
        assert_eq!(in_b.len(), 1);
        assert_eq!(in_a[0].content, "only-in-a");
        assert_eq!(in_b[0].content, "only-in-b");
    }

    #[tokio::test]
    async fn with_kind_and_refs_round_trip() {
        let store = SqliteDiaryStore::open_in_memory().unwrap();
        let entry = DiaryEntry::new("demo", "decision: switch to Rust")
            .with_kind("decision")
            .with_refs(serde_json::json!({"kg_entity": "abc-123"}));
        store.append(entry).await.unwrap();
        let recent = store.recent("demo", 1).await.unwrap();
        assert_eq!(recent[0].kind.as_deref(), Some("decision"));
        assert_eq!(recent[0].refs["kg_entity"], "abc-123");
    }
}
