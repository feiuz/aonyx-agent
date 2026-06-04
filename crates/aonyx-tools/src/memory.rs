//! Memory tools exposed to the LLM (Phase MM): `memory_search`,
//! `memory_diary_append`, `memory_kg_query`.
//!
//! Unlike the fs / git / web tools these are **stateful** — each holds
//! a handle to the project's [`Palace`] (and its project slug), so they
//! can't live in [`crate::ToolRegistry::default_set`]. The CLI builds
//! and registers them at session start, once the palace is open.

use aonyx_core::{AonyxError, MemoryStore, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use aonyx_memory::kg::{Direction, KgStore, SqliteKgStore};
use aonyx_memory::Palace;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

/// `memory_search` — hybrid (BM25) search across the project palace.
pub struct MemorySearch {
    palace: Palace,
}

#[derive(Deserialize)]
struct MemorySearchArgs {
    query: String,
    #[serde(default)]
    k: Option<usize>,
}

impl MemorySearch {
    /// Wrap a palace handle.
    pub fn new(palace: Palace) -> Self {
        Self { palace }
    }
}

#[async_trait]
impl ToolHandler for MemorySearch {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "k": { "type": "integer", "minimum": 1, "maximum": 50, "default": 8 }
            },
            "required": ["query"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: MemorySearchArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("memory_search args: {e}")))?;
        let k = args.k.unwrap_or(8).clamp(1, 50);
        let hits = self.palace.hybrid_search(&args.query, k).await?;
        let results: Vec<Value> = hits
            .into_iter()
            .map(|(content, score)| json!({ "content": content, "score": score }))
            .collect();
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "query": args.query, "results": results }),
            error: None,
        })
    }
}

/// `rag_search` — retrieval over the palace returning **source-attributed**
/// chunks (citations). Hybrid (BM25 + vectors) when an embedder is configured,
/// BM25-only otherwise. Named exactly `rag_search` so `auto_retrieve` picks it
/// up as the local backend (ADR-008) — same contract as the external MCP
/// `<server>__rag_search`.
pub struct RagSearch {
    palace: Palace,
}

#[derive(Deserialize)]
struct RagSearchArgs {
    query: String,
    /// Preferred arg name (matches `auto_retrieve` + the external MCP tool).
    #[serde(default)]
    top_k: Option<usize>,
    /// Accepted alias.
    #[serde(default)]
    k: Option<usize>,
}

impl RagSearch {
    /// Wrap a palace handle (with or without an embedder attached).
    pub fn new(palace: Palace) -> Self {
        Self { palace }
    }
}

#[async_trait]
impl ToolHandler for RagSearch {
    fn name(&self) -> &str {
        "rag_search"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Question or keywords to retrieve from the memory palace." },
                "top_k": { "type": "integer", "minimum": 1, "maximum": 10, "default": 5 }
            },
            "required": ["query"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: RagSearchArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("rag_search args: {e}")))?;
        let k = args.top_k.or(args.k).unwrap_or(5).clamp(1, 10);
        let hits = self.palace.search(&args.query, k).await?;
        let results: Vec<Value> = hits
            .into_iter()
            .map(|sc| {
                json!({
                    "project": sc.chunk.project,
                    "source": sc.chunk.source,
                    "content": sc.chunk.content,
                    "score": sc.score,
                })
            })
            .collect();
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "query": args.query, "results": results }),
            error: None,
        })
    }
}

/// `memory_diary_append` — append a dated note to the project diary.
pub struct MemoryDiaryAppend {
    palace: Palace,
    project: String,
}

#[derive(Deserialize)]
struct MemoryDiaryArgs {
    note: String,
}

impl MemoryDiaryAppend {
    /// Wrap a palace handle scoped to `project`.
    pub fn new(palace: Palace, project: impl Into<String>) -> Self {
        Self {
            palace,
            project: project.into(),
        }
    }
}

#[async_trait]
impl ToolHandler for MemoryDiaryAppend {
    fn name(&self) -> &str {
        "memory_diary_append"
    }

    fn classify(&self) -> SafetyClass {
        // Writes to the palace — reversible, so Caution (not Destructive).
        SafetyClass::Caution
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note": { "type": "string", "description": "Text to append to the project diary." }
            },
            "required": ["note"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: MemoryDiaryArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("memory_diary_append args: {e}")))?;
        self.palace.diary_append(&self.project, &args.note).await?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "appended": true, "chars": args.note.len() }),
            error: None,
        })
    }
}

/// `memory_kg_query` — look up an entity by name in the knowledge graph
/// and return it with its adjacent relations.
pub struct MemoryKgQuery {
    kg: SqliteKgStore,
}

#[derive(Deserialize)]
struct MemoryKgArgs {
    name: String,
}

impl MemoryKgQuery {
    /// Wrap a KG store handle.
    pub fn new(kg: SqliteKgStore) -> Self {
        Self { kg }
    }
}

#[async_trait]
impl ToolHandler for MemoryKgQuery {
    fn name(&self) -> &str {
        "memory_kg_query"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Exact entity name to look up." }
            },
            "required": ["name"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: MemoryKgArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("memory_kg_query args: {e}")))?;
        let entities = self.kg.find_entities_by_name(&args.name).await?;
        let mut out = Vec::new();
        for e in entities {
            let rels = self.kg.relations_for(e.id, Direction::Both).await?;
            let rel_json: Vec<Value> = rels
                .iter()
                .map(|r| {
                    json!({
                        "predicate": r.predicate,
                        "src": r.src_id.to_string(),
                        "dst": r.dst_id.to_string(),
                    })
                })
                .collect();
            out.push(json!({
                "id": e.id.to_string(),
                "name": e.name,
                "entity_type": e.entity_type,
                "attrs": e.attrs,
                "relations": rel_json,
            }));
        }
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "name": args.name, "entities": out }),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_memory::kg::{Entity, Relation};

    #[tokio::test]
    async fn memory_diary_append_writes_to_palace() {
        let palace = Palace::open_in_memory().unwrap();
        let tool = MemoryDiaryAppend::new(palace.clone(), "demo");
        let call = ToolCall {
            id: "1".into(),
            name: "memory_diary_append".into(),
            args: json!({ "note": "remember the milk" }),
        };
        let res = tool.invoke(call).await.unwrap();
        assert_eq!(res.output["appended"], true);
        assert_eq!(tool.classify(), SafetyClass::Caution);
    }

    #[tokio::test]
    async fn memory_search_returns_results_shape() {
        let palace = Palace::open_in_memory().unwrap();
        let tool = MemorySearch::new(palace);
        let call = ToolCall {
            id: "1".into(),
            name: "memory_search".into(),
            args: json!({ "query": "anything", "k": 3 }),
        };
        let res = tool.invoke(call).await.unwrap();
        assert!(res.output["results"].is_array());
        assert_eq!(tool.classify(), SafetyClass::Safe);
    }

    #[tokio::test]
    async fn memory_kg_query_finds_entity_with_relations() {
        let palace = Palace::open_in_memory().unwrap();
        let a = palace
            .kg
            .upsert_entity(Entity::new("Aonyx", "project"))
            .await
            .unwrap();
        let b = palace
            .kg
            .upsert_entity(Entity::new("Damien", "person"))
            .await
            .unwrap();
        palace
            .kg
            .upsert_relation(Relation::new(b, a, "builds"))
            .await
            .unwrap();
        let tool = MemoryKgQuery::new(palace.kg.clone());
        let call = ToolCall {
            id: "1".into(),
            name: "memory_kg_query".into(),
            args: json!({ "name": "Aonyx" }),
        };
        let res = tool.invoke(call).await.unwrap();
        let entities = res.output["entities"].as_array().unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0]["name"], "Aonyx");
        assert_eq!(entities[0]["relations"].as_array().unwrap().len(), 1);
    }
}
