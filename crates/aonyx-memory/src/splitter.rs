//! Tree-sitter AST-aware code chunking.
//!
//! Port target: Aonyx RAG `rag_system/utils/code_splitter.py`.
//!
//! V1 grammars: Python, JavaScript, TypeScript, Rust, Go.
//! Output: one chunk per function / class / method, with `symbol`, `start_line`,
//! `end_line`, `lang` metadata.

// TODO(V1): integrate tree-sitter parsers.
