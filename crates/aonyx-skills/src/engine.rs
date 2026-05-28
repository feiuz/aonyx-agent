//! Trigger matching + system-prompt injection engine.
//!
//! Port target: Aonyx RAG `rag_system/skills/engine.py`.
//!
//! Precedence (highest → lowest): `always_on` > `manual_ids` > `trigger.matches`.

// TODO(V1): match_active(query, project, skills) -> Vec<&Skill>.
