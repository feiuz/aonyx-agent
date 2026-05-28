//! Context-window pressure monitor + summarization.
//!
//! Triggers a summarization pass when `total_tokens >= 0.5 * model_window`.
//! The summarization is carried out by an auxiliary LLM call; orphaned
//! tool-call / tool-result pairs are stripped before summarizing.

// TODO(V1): port Aonyx RAG `agent/compaction.py`.
