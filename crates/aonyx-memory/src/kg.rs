//! Knowledge graph: entities + relations with temporal validity windows.
//!
//! Port reference: Aonyx RAG `rag_system/kg/store.py`.
//!
//! ## Schema (idempotent SQLite migrations)
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS entities (
//!     id TEXT PRIMARY KEY,
//!     name TEXT NOT NULL,
//!     entity_type TEXT NOT NULL,
//!     attrs_json TEXT,
//!     valid_from TEXT,
//!     valid_to TEXT,
//!     source_doc_id TEXT,
//!     confidence REAL NOT NULL DEFAULT 1.0,
//!     created_at TEXT NOT NULL
//! );
//!
//! CREATE TABLE IF NOT EXISTS relations (
//!     id TEXT PRIMARY KEY,
//!     src_id TEXT NOT NULL REFERENCES entities(id),
//!     dst_id TEXT NOT NULL REFERENCES entities(id),
//!     predicate TEXT NOT NULL,
//!     attrs_json TEXT,
//!     valid_from TEXT,
//!     valid_to TEXT,
//!     created_at TEXT NOT NULL
//! );
//! ```
//!
//! Times are stored as RFC 3339 strings so the schema is human-readable in any
//! SQLite client and survives migrations cleanly.

use std::path::Path;
use std::sync::{Arc, Mutex};

use aonyx_core::{AonyxError, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Stable identifier for an [`Entity`].
pub type EntityId = Uuid;

/// Stable identifier for a [`Relation`].
pub type RelationId = Uuid;

/// A node in the knowledge graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Stable id (UUID v4 by default).
    pub id: EntityId,
    /// Human-readable name (`"Damien"`, `"Aonyx Agent"`, `"GPT-5"`).
    pub name: String,
    /// Free-form type tag (`"person"`, `"project"`, `"model"`).
    pub entity_type: String,
    /// Arbitrary structured attributes serialised as JSON.
    #[serde(default)]
    pub attrs: JsonValue,
    /// Lower bound of validity (inclusive). `None` = "since forever".
    pub valid_from: Option<DateTime<Utc>>,
    /// Upper bound of validity (exclusive). `None` = "still true".
    pub valid_to: Option<DateTime<Utc>>,
    /// Optional pointer to the document this entity was extracted from.
    pub source_doc_id: Option<String>,
    /// Confidence in the assertion (0.0–1.0).
    pub confidence: f32,
    /// Wall-clock creation time.
    pub created_at: DateTime<Utc>,
}

impl Entity {
    /// Build a new entity with sensible defaults (`confidence = 1.0`, no validity bounds).
    pub fn new(name: impl Into<String>, entity_type: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            entity_type: entity_type.into(),
            attrs: JsonValue::Null,
            valid_from: None,
            valid_to: None,
            source_doc_id: None,
            confidence: 1.0,
            created_at: Utc::now(),
        }
    }
}

/// An edge in the knowledge graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    /// Stable id.
    pub id: RelationId,
    /// Source entity.
    pub src_id: EntityId,
    /// Destination entity.
    pub dst_id: EntityId,
    /// Free-form predicate (`"works_on"`, `"depends_on"`, `"ports_patterns_from"`).
    pub predicate: String,
    /// Arbitrary structured attributes serialised as JSON.
    #[serde(default)]
    pub attrs: JsonValue,
    /// Lower bound of validity.
    pub valid_from: Option<DateTime<Utc>>,
    /// Upper bound of validity.
    pub valid_to: Option<DateTime<Utc>>,
    /// Wall-clock creation time.
    pub created_at: DateTime<Utc>,
}

impl Relation {
    /// Build a new relation with no validity bounds.
    pub fn new(src_id: EntityId, dst_id: EntityId, predicate: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            src_id,
            dst_id,
            predicate: predicate.into(),
            attrs: JsonValue::Null,
            valid_from: None,
            valid_to: None,
            created_at: Utc::now(),
        }
    }
}

/// Direction selector for relation queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Edges where the entity is `src`.
    Out,
    /// Edges where the entity is `dst`.
    In,
    /// Both directions.
    Both,
}

/// Asynchronous KG store.
#[async_trait]
pub trait KgStore: Send + Sync {
    /// Insert or update an entity, keyed by its `id`.
    async fn upsert_entity(&self, entity: Entity) -> Result<EntityId>;

    /// Insert or update a relation, keyed by its `id`.
    async fn upsert_relation(&self, relation: Relation) -> Result<RelationId>;

    /// Fetch an entity by id.
    async fn get_entity(&self, id: EntityId) -> Result<Option<Entity>>;

    /// Find entities by exact name match (case-sensitive in V1).
    async fn find_entities_by_name(&self, name: &str) -> Result<Vec<Entity>>;

    /// List relations adjacent to an entity.
    async fn relations_for(
        &self,
        entity_id: EntityId,
        direction: Direction,
    ) -> Result<Vec<Relation>>;

    /// Total entity count — cheap sanity check.
    async fn count_entities(&self) -> Result<usize>;
}

/// SQLite-backed [`KgStore`].
///
/// The connection lives behind a `Mutex` and every query runs inside
/// `tokio::task::spawn_blocking`. For V1 this is sufficient; we'll migrate to
/// `tokio-rusqlite` or a connection pool when concurrent writers become a real
/// concern.
#[derive(Clone)]
pub struct SqliteKgStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteKgStore {
    /// Open (or create) the KG database at `path`, running migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .map_err(|e| AonyxError::Memory(format!("open kg db: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database — convenient for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AonyxError::Memory(format!("open in-memory kg: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(MIGRATION_V1)
            .map_err(|e| AonyxError::Memory(format!("migrate kg schema: {e}")))?;
        Ok(())
    }
}

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS entities (
    id            TEXT    PRIMARY KEY,
    name          TEXT    NOT NULL,
    entity_type   TEXT    NOT NULL,
    attrs_json    TEXT,
    valid_from    TEXT,
    valid_to      TEXT,
    source_doc_id TEXT,
    confidence    REAL    NOT NULL DEFAULT 1.0,
    created_at    TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);

CREATE TABLE IF NOT EXISTS relations (
    id          TEXT NOT NULL PRIMARY KEY,
    src_id      TEXT NOT NULL REFERENCES entities(id),
    dst_id      TEXT NOT NULL REFERENCES entities(id),
    predicate   TEXT NOT NULL,
    attrs_json  TEXT,
    valid_from  TEXT,
    valid_to    TEXT,
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_relations_src       ON relations(src_id);
CREATE INDEX IF NOT EXISTS idx_relations_dst       ON relations(dst_id);
CREATE INDEX IF NOT EXISTS idx_relations_predicate ON relations(predicate);
"#;

fn parse_uuid(s: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn parse_ts(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|raw| {
        DateTime::parse_from_rfc3339(&raw)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    })
}

fn entity_from_row(row: &Row<'_>) -> rusqlite::Result<Entity> {
    let id_str: String = row.get(0)?;
    let name: String = row.get(1)?;
    let entity_type: String = row.get(2)?;
    let attrs_json: Option<String> = row.get(3)?;
    let valid_from_raw: Option<String> = row.get(4)?;
    let valid_to_raw: Option<String> = row.get(5)?;
    let source_doc_id: Option<String> = row.get(6)?;
    let confidence: f32 = row.get(7)?;
    let created_at_raw: String = row.get(8)?;

    let attrs = attrs_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(JsonValue::Null);
    let created_at = DateTime::parse_from_rfc3339(&created_at_raw)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(Entity {
        id: parse_uuid(&id_str)?,
        name,
        entity_type,
        attrs,
        valid_from: parse_ts(valid_from_raw),
        valid_to: parse_ts(valid_to_raw),
        source_doc_id,
        confidence,
        created_at,
    })
}

fn relation_from_row(row: &Row<'_>) -> rusqlite::Result<Relation> {
    let id_str: String = row.get(0)?;
    let src_str: String = row.get(1)?;
    let dst_str: String = row.get(2)?;
    let predicate: String = row.get(3)?;
    let attrs_json: Option<String> = row.get(4)?;
    let valid_from_raw: Option<String> = row.get(5)?;
    let valid_to_raw: Option<String> = row.get(6)?;
    let created_at_raw: String = row.get(7)?;

    let attrs = attrs_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(JsonValue::Null);
    let created_at = DateTime::parse_from_rfc3339(&created_at_raw)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(Relation {
        id: parse_uuid(&id_str)?,
        src_id: parse_uuid(&src_str)?,
        dst_id: parse_uuid(&dst_str)?,
        predicate,
        attrs,
        valid_from: parse_ts(valid_from_raw),
        valid_to: parse_ts(valid_to_raw),
        created_at,
    })
}

const ENTITY_COLUMNS: &str =
    "id, name, entity_type, attrs_json, valid_from, valid_to, source_doc_id, confidence, created_at";

const RELATION_COLUMNS: &str =
    "id, src_id, dst_id, predicate, attrs_json, valid_from, valid_to, created_at";

#[async_trait]
impl KgStore for SqliteKgStore {
    async fn upsert_entity(&self, entity: Entity) -> Result<EntityId> {
        let conn = self.conn.clone();
        let id = entity.id;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let lock = conn.lock().expect("kg mutex poisoned");
            lock.execute(
                r#"
                INSERT INTO entities (id, name, entity_type, attrs_json, valid_from, valid_to, source_doc_id, confidence, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(id) DO UPDATE SET
                    name          = excluded.name,
                    entity_type   = excluded.entity_type,
                    attrs_json    = excluded.attrs_json,
                    valid_from    = excluded.valid_from,
                    valid_to      = excluded.valid_to,
                    source_doc_id = excluded.source_doc_id,
                    confidence    = excluded.confidence
                "#,
                params![
                    entity.id.to_string(),
                    entity.name,
                    entity.entity_type,
                    serde_json::to_string(&entity.attrs).ok(),
                    entity.valid_from.map(|d| d.to_rfc3339()),
                    entity.valid_to.map(|d| d.to_rfc3339()),
                    entity.source_doc_id,
                    entity.confidence,
                    entity.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| AonyxError::Memory(format!("upsert_entity: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("kg upsert_entity join: {e}")))??;
        Ok(id)
    }

    async fn upsert_relation(&self, relation: Relation) -> Result<RelationId> {
        let conn = self.conn.clone();
        let id = relation.id;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let lock = conn.lock().expect("kg mutex poisoned");
            lock.execute(
                r#"
                INSERT INTO relations (id, src_id, dst_id, predicate, attrs_json, valid_from, valid_to, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(id) DO UPDATE SET
                    src_id     = excluded.src_id,
                    dst_id     = excluded.dst_id,
                    predicate  = excluded.predicate,
                    attrs_json = excluded.attrs_json,
                    valid_from = excluded.valid_from,
                    valid_to   = excluded.valid_to
                "#,
                params![
                    relation.id.to_string(),
                    relation.src_id.to_string(),
                    relation.dst_id.to_string(),
                    relation.predicate,
                    serde_json::to_string(&relation.attrs).ok(),
                    relation.valid_from.map(|d| d.to_rfc3339()),
                    relation.valid_to.map(|d| d.to_rfc3339()),
                    relation.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| AonyxError::Memory(format!("upsert_relation: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("kg upsert_relation join: {e}")))??;
        Ok(id)
    }

    async fn get_entity(&self, id: EntityId) -> Result<Option<Entity>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Entity>> {
            let lock = conn.lock().expect("kg mutex poisoned");
            let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE id = ?1");
            let mut stmt = lock
                .prepare(&sql)
                .map_err(|e| AonyxError::Memory(format!("prepare get_entity: {e}")))?;
            let row = stmt
                .query_row(params![id.to_string()], entity_from_row)
                .optional()
                .map_err(|e| AonyxError::Memory(format!("get_entity: {e}")))?;
            Ok(row)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("kg get_entity join: {e}")))?
    }

    async fn find_entities_by_name(&self, name: &str) -> Result<Vec<Entity>> {
        let conn = self.conn.clone();
        let needle = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Entity>> {
            let lock = conn.lock().expect("kg mutex poisoned");
            let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE name = ?1");
            let mut stmt = lock
                .prepare(&sql)
                .map_err(|e| AonyxError::Memory(format!("prepare find_entities_by_name: {e}")))?;
            let rows = stmt
                .query_map(params![needle], entity_from_row)
                .map_err(|e| AonyxError::Memory(format!("query find_entities_by_name: {e}")))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(|e| AonyxError::Memory(format!("row decode: {e}")))?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("kg find_entities_by_name join: {e}")))?
    }

    async fn relations_for(
        &self,
        entity_id: EntityId,
        direction: Direction,
    ) -> Result<Vec<Relation>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Relation>> {
            let lock = conn.lock().expect("kg mutex poisoned");
            let where_clause = match direction {
                Direction::Out => "WHERE src_id = ?1",
                Direction::In => "WHERE dst_id = ?1",
                Direction::Both => "WHERE src_id = ?1 OR dst_id = ?1",
            };
            let sql = format!("SELECT {RELATION_COLUMNS} FROM relations {where_clause}");
            let mut stmt = lock
                .prepare(&sql)
                .map_err(|e| AonyxError::Memory(format!("prepare relations_for: {e}")))?;
            let rows = stmt
                .query_map(params![entity_id.to_string()], relation_from_row)
                .map_err(|e| AonyxError::Memory(format!("query relations_for: {e}")))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(|e| AonyxError::Memory(format!("row decode: {e}")))?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("kg relations_for join: {e}")))?
    }

    async fn count_entities(&self) -> Result<usize> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<usize> {
            let lock = conn.lock().expect("kg mutex poisoned");
            let n: i64 = lock
                .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
                .map_err(|e| AonyxError::Memory(format!("count_entities: {e}")))?;
            Ok(n.max(0) as usize)
        })
        .await
        .map_err(|e| AonyxError::Memory(format!("kg count_entities join: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_in_memory_runs_migrations() {
        let store = SqliteKgStore::open_in_memory().expect("open in-memory");
        assert_eq!(store.count_entities().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn upsert_and_fetch_entity() {
        let store = SqliteKgStore::open_in_memory().expect("open in-memory");
        let e = Entity::new("Damien", "person");
        let id = store.upsert_entity(e.clone()).await.unwrap();
        let got = store.get_entity(id).await.unwrap().expect("entity exists");
        assert_eq!(got.name, "Damien");
        assert_eq!(got.entity_type, "person");
        assert_eq!(got.confidence, 1.0);
    }

    #[tokio::test]
    async fn upsert_is_idempotent() {
        let store = SqliteKgStore::open_in_memory().expect("open in-memory");
        let mut e = Entity::new("Aonyx Agent", "project");
        let id = store.upsert_entity(e.clone()).await.unwrap();
        e.name = "Aonyx Agent (renamed)".into();
        e.id = id;
        store.upsert_entity(e).await.unwrap();
        assert_eq!(store.count_entities().await.unwrap(), 1);
        let got = store.get_entity(id).await.unwrap().expect("entity exists");
        assert_eq!(got.name, "Aonyx Agent (renamed)");
    }

    #[tokio::test]
    async fn find_by_name_returns_matching_entities() {
        let store = SqliteKgStore::open_in_memory().expect("open in-memory");
        store
            .upsert_entity(Entity::new("Alice", "person"))
            .await
            .unwrap();
        store
            .upsert_entity(Entity::new("Bob", "person"))
            .await
            .unwrap();
        let hits = store.find_entities_by_name("Alice").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Alice");
    }

    #[tokio::test]
    async fn relations_can_be_queried_in_both_directions() {
        let store = SqliteKgStore::open_in_memory().expect("open in-memory");
        let a_id = store
            .upsert_entity(Entity::new("Aonyx Agent", "project"))
            .await
            .unwrap();
        let b_id = store
            .upsert_entity(Entity::new("Aonyx RAG", "project"))
            .await
            .unwrap();
        store
            .upsert_relation(Relation::new(a_id, b_id, "ports_patterns_from"))
            .await
            .unwrap();

        let out = store.relations_for(a_id, Direction::Out).await.unwrap();
        let into = store.relations_for(b_id, Direction::In).await.unwrap();
        let both = store.relations_for(a_id, Direction::Both).await.unwrap();

        assert_eq!(out.len(), 1);
        assert_eq!(into.len(), 1);
        assert_eq!(both.len(), 1);
        assert_eq!(out[0].predicate, "ports_patterns_from");
        assert_eq!(out[0].src_id, a_id);
        assert_eq!(out[0].dst_id, b_id);
    }
}
