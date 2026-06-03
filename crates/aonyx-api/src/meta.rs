//! Tools / skills / config metadata endpoints + the OpenAPI document.
//!
//! The tool/skill/config data comes from the injected
//! [`ApiAgent`](crate::ApiAgent) (the binary fills it from its live registry,
//! loaded skills, and config). Secrets are never included.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::agent::{ConfigInfo, SkillInfo, ToolInfo};
use crate::state::ApiState;

/// `GET /v1/tools` — list the registered tools.
pub async fn list_tools(State(state): State<ApiState>) -> Json<Vec<ToolInfo>> {
    Json(state.agent.tools())
}

/// `GET /v1/skills` — list the loaded skills.
pub async fn list_skills(State(state): State<ApiState>) -> Json<Vec<SkillInfo>> {
    Json(state.agent.skills())
}

/// `GET /v1/config` — the non-secret config snapshot.
pub async fn get_config(State(state): State<ApiState>) -> Json<ConfigInfo> {
    Json(state.agent.config())
}

/// `GET /v1/openapi.json` — a hand-written OpenAPI 3.0 description (open, no
/// auth, so integrators can discover the surface).
pub async fn openapi() -> Json<Value> {
    Json(spec())
}

fn id_param() -> Value {
    json!({
        "name": "id", "in": "path", "required": true,
        "schema": { "type": "string", "format": "uuid" }
    })
}

fn spec() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Aonyx Agent API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "REST + WebSocket automation API over the Aonyx Agent core."
        },
        "servers": [{ "url": "/" }],
        "components": {
            "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } }
        },
        "security": [{ "bearer": [] }],
        "paths": {
            "/v1/health": { "get": {
                "summary": "Liveness probe (open)", "security": [],
                "responses": { "200": { "description": "ok" } } } },
            "/v1/info": { "get": {
                "summary": "Server identity + capabilities",
                "responses": { "200": { "description": "server info" } } } },
            "/v1/sessions": {
                "get": {
                    "summary": "List sessions",
                    "parameters": [
                        { "name": "project", "in": "query", "schema": { "type": "string" } },
                        { "name": "limit", "in": "query", "schema": { "type": "integer" } }
                    ],
                    "responses": { "200": { "description": "session summaries" } } },
                "post": {
                    "summary": "Create a session",
                    "responses": { "201": { "description": "created session" } } }
            },
            "/v1/sessions/{id}": {
                "get": {
                    "summary": "Get a session (with messages)",
                    "parameters": [ id_param() ],
                    "responses": { "200": { "description": "session" }, "404": { "description": "not found" } } },
                "delete": {
                    "summary": "Delete a session",
                    "parameters": [ id_param() ],
                    "responses": { "204": { "description": "deleted" } } }
            },
            "/v1/sessions/{id}/messages": { "post": {
                "summary": "Run one blocking turn",
                "parameters": [ id_param() ],
                "responses": { "200": { "description": "turn result" } } } },
            "/v1/sessions/{id}/messages/stream": { "post": {
                "summary": "Run one turn, streaming SSE frames",
                "parameters": [ id_param() ],
                "responses": { "200": { "description": "text/event-stream" } } } },
            "/v1/sessions/{id}/stream": { "get": {
                "summary": "Bidirectional WebSocket turn stream",
                "parameters": [ id_param() ],
                "responses": { "101": { "description": "switching protocols" } } } },
            "/v1/memory/search": { "get": {
                "summary": "Hybrid memory search",
                "parameters": [
                    { "name": "q", "in": "query", "required": true, "schema": { "type": "string" } },
                    { "name": "k", "in": "query", "schema": { "type": "integer" } }
                ],
                "responses": { "200": { "description": "hits" } } } },
            "/v1/memory/diary": {
                "get": { "summary": "Recent diary entries", "responses": { "200": { "description": "entries" } } },
                "post": { "summary": "Append a diary entry", "responses": { "201": { "description": "created" } } }
            },
            "/v1/memory/kg/entities": { "get": {
                "summary": "List KG entities", "responses": { "200": { "description": "entities" } } } },
            "/v1/memory/kg/entities/{name}": { "get": {
                "summary": "Entities matching a name",
                "parameters": [ { "name": "name", "in": "path", "required": true, "schema": { "type": "string" } } ],
                "responses": { "200": { "description": "entities" } } } },
            "/v1/memory/kg/relations": { "get": {
                "summary": "List KG relations", "responses": { "200": { "description": "relations" } } } },
            "/v1/tools": { "get": { "summary": "List tools", "responses": { "200": { "description": "tools" } } } },
            "/v1/skills": { "get": { "summary": "List skills", "responses": { "200": { "description": "skills" } } } },
            "/v1/config": { "get": { "summary": "Non-secret config", "responses": { "200": { "description": "config" } } } },
            "/v1/openapi.json": { "get": { "summary": "This document", "security": [],
                "responses": { "200": { "description": "OpenAPI 3.0 spec" } } } }
        }
    })
}
