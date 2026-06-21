//! Memory-palace endpoints: hybrid search, diary read/append, and KG browse.
//!
//! These talk to [`Palace`](aonyx_memory::Palace) directly (held in
//! [`ApiState`](crate::ApiState)); no agent loop is involved.

use aonyx_core::MemoryStore;
use aonyx_memory::{ChunksStore, DiaryEntry, DiaryStore, Entity, KgStore, Relation};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::state::ApiState;

/// Query for `GET /v1/memory/search`.
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    /// The search query (required).
    pub q: String,
    /// Number of hits to return (default 10).
    pub k: Option<usize>,
}

/// One hybrid-search hit.
#[derive(Debug, Serialize)]
pub struct SearchHit {
    /// Matched chunk content.
    pub content: String,
    /// Fusion score (higher is better).
    pub score: f32,
}

/// `GET /v1/memory/search?q=&k=` — hybrid (BM25 + vectors) search.
pub async fn search(
    State(state): State<ApiState>,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<Vec<SearchHit>>> {
    if params.q.trim().is_empty() {
        return Err(ApiError::BadRequest("missing query `q`".into()));
    }
    let k = params.k.unwrap_or(10).clamp(1, 100);
    let hits = state.palace.hybrid_search(&params.q, k).await?;
    Ok(Json(
        hits.into_iter()
            .map(|(content, score)| SearchHit { content, score })
            .collect(),
    ))
}

/// Body for `POST /v1/memory/ingest`.
#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    /// Project to ingest into; defaults to the server project.
    pub project: Option<String>,
    /// Source label for the document (file path, title, …).
    pub source: String,
    /// The raw text to chunk, embed, and store.
    pub text: String,
}

/// `POST /v1/memory/ingest` — chunk + embed a document into the project's palace
/// so `rag_search` / `memory_search` find it. Returns the chunk count.
pub async fn ingest(
    State(state): State<ApiState>,
    Json(req): Json<IngestRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    if req.text.trim().is_empty() {
        return Err(ApiError::BadRequest("empty text".into()));
    }
    let source = if req.source.trim().is_empty() {
        "uploaded".to_string()
    } else {
        req.source.clone()
    };
    let project = state.project_or_default(req.project);
    let chunks = state.palace.ingest_text(&project, &source, &req.text).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "project": project, "source": source, "chunks": chunks })),
    ))
}

/// One project's memory footprint.
#[derive(Debug, Serialize)]
pub struct ProjectInfo {
    /// Project slug.
    pub project: String,
    /// Number of ingested chunks in this project.
    pub chunks: usize,
}

/// `GET /v1/memory/projects` — distinct memory projects with their chunk counts.
pub async fn projects(State(state): State<ApiState>) -> ApiResult<Json<Vec<ProjectInfo>>> {
    let rows = state.palace.chunks.projects().await?;
    Ok(Json(
        rows.into_iter()
            .map(|(project, chunks)| ProjectInfo { project, chunks })
            .collect(),
    ))
}

/// Query for `GET /v1/memory/diary`.
#[derive(Debug, Deserialize)]
pub struct DiaryParams {
    /// Project slug; defaults to the server project.
    pub project: Option<String>,
    /// Max entries (default 50).
    pub limit: Option<usize>,
}

/// `GET /v1/memory/diary?project=&limit=` — recent diary entries (newest
/// first).
pub async fn diary_list(
    State(state): State<ApiState>,
    Query(params): Query<DiaryParams>,
) -> ApiResult<Json<Vec<DiaryEntry>>> {
    let project = state.project_or_default(params.project);
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let entries = state.palace.diary.recent(&project, limit).await?;
    Ok(Json(entries))
}

/// Body for `POST /v1/memory/diary`.
#[derive(Debug, Deserialize)]
pub struct DiaryAppendRequest {
    /// Project slug; defaults to the server project.
    pub project: Option<String>,
    /// The diary entry text.
    pub content: String,
}

/// `POST /v1/memory/diary` — append a diary entry.
pub async fn diary_append(
    State(state): State<ApiState>,
    Json(req): Json<DiaryAppendRequest>,
) -> ApiResult<StatusCode> {
    if req.content.trim().is_empty() {
        return Err(ApiError::BadRequest("empty diary content".into()));
    }
    let project = state.project_or_default(req.project);
    state.palace.diary_append(&project, &req.content).await?;
    Ok(StatusCode::CREATED)
}

/// Query for the KG listing endpoints.
#[derive(Debug, Deserialize)]
pub struct KgParams {
    /// Max rows (default 100).
    pub limit: Option<usize>,
}

/// `GET /v1/memory/kg/entities?limit=` — list entities (newest first).
pub async fn kg_entities(
    State(state): State<ApiState>,
    Query(params): Query<KgParams>,
) -> ApiResult<Json<Vec<Entity>>> {
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    Ok(Json(state.palace.kg.list_entities(limit).await?))
}

/// `GET /v1/memory/kg/entities/:name` — entities matching a name.
pub async fn kg_entity_by_name(
    State(state): State<ApiState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<Entity>>> {
    Ok(Json(state.palace.kg.find_entities_by_name(&name).await?))
}

/// `GET /v1/memory/kg/relations?limit=` — list relations (newest first).
pub async fn kg_relations(
    State(state): State<ApiState>,
    Query(params): Query<KgParams>,
) -> ApiResult<Json<Vec<Relation>>> {
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    Ok(Json(state.palace.kg.list_relations(limit).await?))
}
