//! Lightweight intent classifier.
//!
//! Routes the user message into one of:
//! `Chitchat | Task | Recall | Code | Research`. The chosen bucket selects a
//! different prompt template and tool subset.

// TODO(V1): port Aonyx RAG `agent/classifier.py`.
