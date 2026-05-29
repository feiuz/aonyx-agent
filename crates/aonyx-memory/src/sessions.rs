//! Cross-run session persistence.
//!
//! Sessions live in a single SQLite file (typically `~/.aonyx/sessions.db`)
//! shared across every project. The `project` column scopes lookups so the
//! TUI can show "sessions for *this* project" without polluting the listing
//! with unrelated work.
//!
//! ## Schema (idempotent)
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS sessions (
//!     id            TEXT PRIMARY KEY,
//!     project       TEXT NOT NULL,
//!     created_at    TEXT NOT NULL,
//!     updated_at    TEXT NOT NULL,
//!     parent_id     TEXT,
//!     title         TEXT NOT NULL,
//!     turns         INTEGER NOT NULL DEFAULT 0,
//!     messages_json TEXT NOT NULL
//! );
//! CREATE INDEX IF NOT EXISTS idx_sessions_project_updated
//!     ON sessions(project, updated_at DESC);
//! ```

use std::path::Path;
use std::sync::{Arc, Mutex};

use aonyx_core::{AonyxError, Message, Result, Role};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a [`SessionRecord`].
pub type SessionId = Uuid;

/// One row of the `sessions` table, hydrated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Stable id.
    pub id: SessionId,
    /// Project slug (typically the cwd directory name).
    pub project: String,
    /// First write of the row.
    pub created_at: DateTime<Utc>,
    /// Last update of the row.
    pub updated_at: DateTime<Utc>,
    /// Parent session this one was forked from, if any.
    pub parent_id: Option<SessionId>,
    /// One-line title — derived from the first user message.
    pub title: String,
    /// Number of completed turns.
    pub turns: u32,
    /// Full message log.
    pub messages: Vec<Message>,
}

/// Async session store.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Create a new session row and return the hydrated record.
    async fn create(&self, project: &str, messages: Vec<Message>) -> Result<SessionRecord>;

    /// Replace the messages of an existing session and bump `turns` + `updated_at`.
    async fn update(&self, id: SessionId, messages: Vec<Message>, turns: u32) -> Result<()>;

    /// Most recent sessions for `project` first.
    async fn list_by_project(&self, project: &str, limit: usize) -> Result<Vec<SessionRecord>>;

    /// Fetch by id.
    async fn get(&self, id: SessionId) -> Result<Option<SessionRecord>>;

    /// Remove a session.
    async fn delete(&self, id: SessionId) -> Result<()>;

    /// Most recent session for `project`, if any.
    async fn latest(&self, project: &str) -> Result<Option<SessionRecord>>;
}

/// SQLite-backed [`SessionStore`].
#[derive(Clone)]
pub struct SqliteSessionStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSessionStore {
    /// Open (or create) the sessions database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .map_err(|e| AonyxError::Memory(format!("open sessions db: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database — convenient for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AonyxError::Memory(format!("open in-memory sessions: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(MIGRATION_V1)
            .map_err(|e| AonyxError::Memory(format!("migrate sessions schema: {e}")))?;
        Ok(())
    }
}

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id            TEXT PRIMARY KEY,
    project       TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    parent_id     TEXT,
    title         TEXT NOT NULL,
    turns         INTEGER NOT NULL DEFAULT 0,
    messages_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_project_updated
    ON sessions(project, updated_at DESC);
"#;

const COLUMNS: &str = "id, project, created_at, updated_at, parent_id, title, turns, messages_json";

fn extract_title(messages: &[Message]) -> String {
    let raw = messages
        .iter()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.trim().to_string())
        .unwrap_or_else(|| "new session".to_string());
    let single_line = raw.replace('\n', " ");
    if single_line.chars().count() > 60 {
        let cut: String = single_line.chars().take(60).collect();
        format!("{cut}…")
    } else if single_line.is_empty() {
        "new session".to_string()
    } else {
        single_line
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let id_str: String = row.get(0)?;
    let project: String = row.get(1)?;
    let created_raw: String = row.get(2)?;
    let updated_raw: String = row.get(3)?;
    let parent_raw: Option<String> = row.get(4)?;
    let title: String = row.get(5)?;
    let turns: i64 = row.get(6)?;
    let messages_raw: String = row.get(7)?;

    let id = Uuid::parse_str(&id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let parent_id = parent_raw
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let created_at = DateTime::parse_from_rfc3339(&created_raw)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let updated_at = DateTime::parse_from_rfc3339(&updated_raw)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let messages: Vec<Message> = serde_json::from_str(&messages_raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(SessionRecord {
        id,
        project,
        created_at,
        updated_at,
        parent_id,
        title,
        turns: turns.max(0) as u32,
        messages,
    })
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, project: &str, messages: Vec<Message>) -> Result<SessionRecord> {
        let conn = self.conn.clone();
        let project = project.to_string();
        let record = SessionRecord {
            id: Uuid::new_v4(),
            project: project.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            parent_id: None,
            title: extract_title(&messages),
            turns: 0,
            messages,
        };
        let to_insert = record.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let lock = conn.lock().expect("sessions mutex poisoned");
            let json = serde_json::to_string(&to_insert.messages)
                .map_err(|e| AonyxError::Memory(format!("encode messages: {e}")))?;
            lock.execute(
                &format!(
                    "INSERT INTO sessions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
                ),
                params![
                    to_insert.id.to_string(),
                    to_insert.project,
                    to_insert.created_at.to_rfc3339(),
                    to_insert.updated_at.to_rfc3339(),
                    to_insert.parent_id.map(|u| u.to_string()),
                    to_insert.title,
                    to_insert.turns as i64,
                    json,
                ],
            )
            .map_err(|e| AonyxError::Memory(format!("insert session: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("create join: {e}")))??;
        Ok(record)
    }

    async fn update(&self, id: SessionId, messages: Vec<Message>, turns: u32) -> Result<()> {
        let conn = self.conn.clone();
        let title = extract_title(&messages);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let lock = conn.lock().expect("sessions mutex poisoned");
            let json = serde_json::to_string(&messages)
                .map_err(|e| AonyxError::Memory(format!("encode messages: {e}")))?;
            lock.execute(
                "UPDATE sessions
                    SET updated_at = ?2, messages_json = ?3, turns = ?4, title = ?5
                    WHERE id = ?1",
                params![
                    id.to_string(),
                    Utc::now().to_rfc3339(),
                    json,
                    turns as i64,
                    title,
                ],
            )
            .map_err(|e| AonyxError::Memory(format!("update session: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("update join: {e}")))?
    }

    async fn list_by_project(&self, project: &str, limit: usize) -> Result<Vec<SessionRecord>> {
        let conn = self.conn.clone();
        let project = project.to_string();
        let limit = limit as i64;
        tokio::task::spawn_blocking(move || -> Result<Vec<SessionRecord>> {
            let lock = conn.lock().expect("sessions mutex poisoned");
            let mut stmt = lock
                .prepare(&format!(
                    "SELECT {COLUMNS} FROM sessions
                     WHERE project = ?1
                     ORDER BY updated_at DESC
                     LIMIT ?2"
                ))
                .map_err(|e| AonyxError::Memory(format!("prepare list: {e}")))?;
            let rows = stmt
                .query_map(params![project, limit], row_to_record)
                .map_err(|e| AonyxError::Memory(format!("query list: {e}")))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(|e| AonyxError::Memory(format!("row decode: {e}")))?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("list join: {e}")))?
    }

    async fn get(&self, id: SessionId) -> Result<Option<SessionRecord>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<SessionRecord>> {
            let lock = conn.lock().expect("sessions mutex poisoned");
            let mut stmt = lock
                .prepare(&format!("SELECT {COLUMNS} FROM sessions WHERE id = ?1"))
                .map_err(|e| AonyxError::Memory(format!("prepare get: {e}")))?;
            stmt.query_row(params![id.to_string()], row_to_record)
                .optional()
                .map_err(|e| AonyxError::Memory(format!("get session: {e}")))
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("get join: {e}")))?
    }

    async fn delete(&self, id: SessionId) -> Result<()> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let lock = conn.lock().expect("sessions mutex poisoned");
            lock.execute(
                "DELETE FROM sessions WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(|e| AonyxError::Memory(format!("delete session: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("delete join: {e}")))?
    }

    async fn latest(&self, project: &str) -> Result<Option<SessionRecord>> {
        let list = self.list_by_project(project, 1).await?;
        Ok(list.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::Role;

    fn msg(role: Role, content: &str) -> Message {
        Message::new(role, content.to_string())
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let store = SqliteSessionStore::open_in_memory().unwrap();
        let messages = vec![msg(Role::System, "be brief"), msg(Role::User, "hello")];
        let created = store.create("demo", messages.clone()).await.unwrap();
        let got = store.get(created.id).await.unwrap().expect("found");
        assert_eq!(got.project, "demo");
        assert_eq!(got.title, "hello");
        assert_eq!(got.messages.len(), 2);
        assert_eq!(got.turns, 0);
    }

    #[tokio::test]
    async fn update_bumps_turns_and_title() {
        let store = SqliteSessionStore::open_in_memory().unwrap();
        let created = store
            .create("demo", vec![msg(Role::User, "first")])
            .await
            .unwrap();
        let new_msgs = vec![
            msg(Role::User, "second user query that drives the title"),
            msg(Role::Assistant, "ok"),
        ];
        store.update(created.id, new_msgs, 1).await.unwrap();
        let got = store.get(created.id).await.unwrap().unwrap();
        assert_eq!(got.turns, 1);
        assert!(got.title.starts_with("second user"));
    }

    #[tokio::test]
    async fn list_orders_by_updated_desc_and_scopes_project() {
        let store = SqliteSessionStore::open_in_memory().unwrap();
        let _a = store
            .create("demo", vec![msg(Role::User, "older")])
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let b = store
            .create("demo", vec![msg(Role::User, "newer")])
            .await
            .unwrap();
        let _c = store
            .create("other", vec![msg(Role::User, "wrong project")])
            .await
            .unwrap();

        let list = store.list_by_project("demo", 10).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, b.id);
    }

    #[tokio::test]
    async fn latest_returns_most_recent_for_project() {
        let store = SqliteSessionStore::open_in_memory().unwrap();
        let _ = store
            .create("demo", vec![msg(Role::User, "old")])
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let recent = store
            .create("demo", vec![msg(Role::User, "fresh")])
            .await
            .unwrap();

        let latest = store.latest("demo").await.unwrap().unwrap();
        assert_eq!(latest.id, recent.id);
        assert!(store.latest("nothing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_removes_a_session() {
        let store = SqliteSessionStore::open_in_memory().unwrap();
        let s = store
            .create("demo", vec![msg(Role::User, "x")])
            .await
            .unwrap();
        store.delete(s.id).await.unwrap();
        assert!(store.get(s.id).await.unwrap().is_none());
    }

    #[test]
    fn extract_title_truncates_long_first_user_message() {
        let m = vec![msg(Role::User, &"a".repeat(200))];
        let title = extract_title(&m);
        assert!(title.chars().count() <= 61);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn extract_title_collapses_newlines() {
        let m = vec![msg(Role::User, "line one\nline two\nline three")];
        let title = extract_title(&m);
        assert!(!title.contains('\n'));
        assert!(title.contains("line one"));
    }
}
