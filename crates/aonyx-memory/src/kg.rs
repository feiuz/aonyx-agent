//! Knowledge graph: entities + relations with temporal validity windows.
//!
//! Port target: Aonyx RAG `rag_system/kg/store.py` (entities, relations,
//! `valid_from`, `valid_to`, confidence, source_doc_id).
//!
//! V1 schema (SQLite, idempotent migrations):
//! ```sql
//! CREATE TABLE IF NOT EXISTS entities (
//!     id TEXT PRIMARY KEY,
//!     name TEXT NOT NULL,
//!     type TEXT NOT NULL,
//!     attrs_json TEXT,
//!     valid_from TEXT,
//!     valid_to TEXT,
//!     source_doc_id TEXT,
//!     confidence REAL,
//!     created_at TEXT NOT NULL
//! );
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

// TODO(V1): implement KgStore against rusqlite.
