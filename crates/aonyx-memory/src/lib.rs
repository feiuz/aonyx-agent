//! # aonyx-memory
//!
//! The **memory palace** — Aonyx Agent's differentiator vs flat-file agent memories.
//!
//! ## Subsystems (V1 target)
//! - [`kg`] — Knowledge Graph with temporal validity windows.
//! - [`diary`] — Append-only narrative log per project.
//! - [`hybrid`] — BM25 + vectors + RRF fusion with temporal boost.
//! - [`splitter`] — Tree-sitter AST-aware code chunking.
//! - [`cross_link`] — Inter-project semantic linking via centroid cosine.
//! - [`time_machine`] — `as_of` queries over the full store.
//!
//! ## Storage layout
//! - `~/.aonyx/sessions.db` — cross-project session history (FTS5).
//! - `./.aonyx/palace.db` — per-project KG, diary, chunks, embeddings.
//!
//! The current crate ships scaffolded module skeletons and an in-memory
//! [`InMemoryStore`] suitable for tests; production backends land iteratively.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod chunks;
pub mod cross_link;
pub mod diary;
pub mod hybrid;
pub mod kg;
pub mod palace;
pub mod sessions;
pub mod splitter;
pub mod time_machine;

mod inmem;

pub use chunks::{Chunk, ChunkId, ChunksStore, ScoredChunk, SqliteChunksStore};
pub use diary::{DiaryEntry, DiaryEntryId, DiaryStore, SqliteDiaryStore};
pub use inmem::InMemoryStore;
pub use kg::{Direction, Entity, EntityId, KgStore, Relation, RelationId, SqliteKgStore};
pub use palace::Palace;
pub use sessions::{SessionId, SessionRecord, SessionStore, SqliteSessionStore};
