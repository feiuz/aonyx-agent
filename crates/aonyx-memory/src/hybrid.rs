//! Hybrid search: BM25 + vectors + RRF fusion with temporal boost.
//!
//! Port target: Aonyx RAG `rag_system/utils/hybrid_search.py` and `utils/bm25_store.py`.
//!
//! Implementation plan:
//! 1. SQLite FTS5 for BM25 with a custom tokenizer for technical identifiers.
//! 2. `fastembed-rs` (ONNX) for embeddings, `hnsw_rs` for ANN search.
//! 3. RRF fusion with `k = 60`, plus exponential decay temporal boost.

// TODO(V1): hybrid_search(query, mode, k, as_of) -> Vec<ScoredChunk>.
