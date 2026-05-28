# BMAD — Architecture (Architect)

**Project**: Aonyx Agent
**Phase**: 3 — Architecture
**Date**: 2026-05-28
**Status**: Draft v0.1

---

## High-level diagram

```
                              ┌──────────────────────────┐
                              │       aonyx (CLI)         │
                              │   binary entrypoint       │
                              └─────────────┬────────────┘
                                            │
                              ┌─────────────▼────────────┐
                              │     aonyx-agent           │
                              │   Loop · Compaction       │
                              │   Approval · Subagent     │
                              └──┬────┬────┬────┬─────────┘
                                 │    │    │    │
              ┌──────────────────┘    │    │    └──────────────────┐
              │                       │    │                        │
       ┌──────▼───────┐       ┌──────▼─┐  └──┐               ┌─────▼──────┐
       │ aonyx-llm    │       │ aonyx- │     │               │  aonyx-mcp │
       │ Provider     │       │ tools  │     │               │ Client +   │
       │ router       │       │ fs/git │     │               │ Server     │
       └──────────────┘       └────────┘     │               └────────────┘
                                             │
                              ┌──────────────▼────────────────────┐
                              │            aonyx-skills              │
                              │     SKILL.md engine · auto-gen        │
                              └──────────────┬────────────────────┘
                                             │
                              ┌──────────────▼────────────────────┐
                              │           aonyx-memory ⭐             │
                              │   KG · Diary · Hybrid · Time-machine  │
                              │     SQLite (FTS5) + HNSW (fastembed)   │
                              └────────────────────────────────────────┘

                              ┌────────────────────────────────────────┐
                              │              aonyx-core                  │
                              │   Shared types · traits · errors          │
                              └────────────────────────────────────────┘
```

## Crate-by-crate

### `aonyx-core`
- Domain types: `Message`, `ToolCall`, `ToolResult`, `Session`, `SkillRef`.
- Traits: `LlmProvider`, `MemoryStore`, `ToolHandler`, `SkillSource`.
- Canonical error type: `AonyxError`, `Result<T> = std::result::Result<T, AonyxError>`.
- Zero I/O. Pure types.

### `aonyx-memory` ⭐
- **Storage layer**: `rusqlite` bundled SQLite. One DB per project (`./.aonyx/palace.db`) + one global session DB (`~/.aonyx/sessions.db`).
- **Schema** (SQL, idempotent migrations):
  - `entities(id, name, type, attrs_json, valid_from, valid_to, source_doc_id, confidence, created_at)`
  - `relations(id, src_id, dst_id, predicate, attrs_json, valid_from, valid_to, created_at)`
  - `documents(id, project, source, content, lang, metadata_json, ts, embedding BLOB)`
  - `chunks(id, doc_id, ord, content, symbol, lang, start_line, end_line, ts, embedding BLOB)`
  - `chunks_fts` (FTS5 virtual table over `content`)
  - `diary(id, project, ts, content, kind, refs_json)`
  - `cross_links(src_project, dst_project, score, computed_at)`
- **Indexing**:
  - BM25: native via FTS5 with custom tokenizer for identifiers (`JWT_SECRET`, `kebab-case-id`).
  - Vectors: `fastembed-rs` (ONNX, MiniLM-L6) + `hnsw_rs` index serialized to disk.
- **Search**:
  - `hybrid_search(query, mode, k, as_of?)` → BM25 top-N + vector top-N → **RRF** (k=60) + temporal boost.
  - `time_machine(as_of)` filters every search by `valid_from <= as_of <= valid_to`.
- **Code-aware splitter**:
  - `tree-sitter` + `tree-sitter-python`, `-javascript`, `-typescript`, `-rust`, `-go`.
  - 1 chunk = 1 function/class/method, with `symbol`, `start_line`, `end_line` metadata.
- **Cross-linking**:
  - Project centroid = mean(embeddings of chunks). Cosine across projects, recomputed lazily.
- **Public API**: async traits — `MemoryStore`, `KgStore`, `DiaryStore`, `Indexer`.

### `aonyx-llm`
- Trait `LlmProvider`:
  - `chat_stream(req: ChatRequest) -> impl Stream<Item = ChatChunk>`
  - `tool_call_parse(chunk: &ChatChunk) -> Option<ToolCall>`
- Implementations:
  - `AnthropicProvider` (native Anthropic API)
  - `OpenAiProvider`
  - `OpenRouterProvider` (OpenAI-compat with model routing header)
  - `OllamaProvider` (`/api/chat`)
  - `LmStudioProvider` (OpenAI-compat custom base URL)
  - `NousPortalProvider` (Nous Portal endpoint)
- **Router**: configured chain `[primary, fallback1, fallback2]` with per-error retry policy.
- Streaming via `eventsource-stream` for SSE providers and chunked JSON for Anthropic.

### `aonyx-tools`
- Trait `ToolHandler`:
  - `fn name(&self) -> &str`
  - `fn schema(&self) -> JsonSchema`
  - `async fn invoke(&self, args: Value) -> Result<Value>`
  - `fn classify(&self) -> SafetyClass` — `Safe` / `Caution` / `Destructive`
- Registry: `ToolRegistry::default_set()` returns the V1 18 built-ins.
- Each tool is a small module with its own tests.

### `aonyx-skills`
- `Skill` struct = parsed `SKILL.md` (frontmatter YAML + markdown body).
- `SkillLoader::load_dir(path)` walks for `**/SKILL.md`.
- Trigger matching engine (port of Aonyx RAG `engine.py`).
- Auto-generation: after a successful run with ≥3 occurrences of the same task shape, the agent drafts a candidate `SKILL.md` proposal which the user reviews/accepts.

### `aonyx-agent`
- `AgentRunner::run(session_id, user_msg)` — the main loop.
- Subsystems:
  - `Compactor`: monitors `total_tokens`, triggers summarization at 50 % of context window.
  - `Classifier`: routes user msg → `[chitchat | task | recall | code | research]` to pick the prompt template.
  - `ApprovalGate`: intercepts `Destructive` tool calls, prompts in CLI / returns error in non-interactive.
  - `Subagent` (V2): spawn isolated child agent with whitelist tool set.
- Observability: every iteration writes a structured `tracing` span.

### `aonyx-mcp`
- Built on `rmcp` (or vendored minimal subset).
- `McpClient::connect(url_or_stdio)` registers external tools into `ToolRegistry`.
- `McpServer::serve(port)` exposes `aonyx-tools` + `aonyx-memory` tools to other agents (Claude Code, etc.).
- Auth: Bearer token from `~/.aonyx/mcp_tokens.toml`.

### `aonyx-cli`
- `clap` derive structure:
  - `aonyx` (default: open interactive session)
  - `aonyx new <path>`
  - `aonyx resume [session_id]`
  - `aonyx config {get,set,list}`
  - `aonyx memory {stats,search,export,import}`
  - `aonyx skills {list,install,enable,disable}`
  - `aonyx mcp {serve,connect}`
- Slash commands inside session: `/new /resume /model /provider /skill /undo /clear /quit`.

### `aonyx-tui` (lib-only V1, full V1.5)
- `ratatui` + `crossterm`.
- Components: message viewport, tool log, status bar, sticky composer.

### `aonyx-adapters` (V2)
- `telegram::TelegramAdapter` (`teloxide`)
- `discord::DiscordAdapter` (`serenity`)
- `openai_server::OpenAiCompatServer` (`axum`)
- All implement a `ConversationAdapter` trait so the agent loop is unaware of the channel.

## Data flows

### 1. First-time install
1. User: `cargo install aonyx-agent`
2. First run: `aonyx` detects missing `~/.aonyx/config.toml`, runs setup wizard.
3. Wizard: choose provider (Anthropic / OpenAI / Ollama-download), enter API key (stored in OS keyring), choose default model.
4. Creates `~/.aonyx/sessions.db`, registers default tools, writes config.

### 2. A turn
1. User input → `aonyx-cli` → `AgentRunner::turn()`.
2. Pre-turn: `aonyx-memory.recall(user_msg, k=10)` → injects top-N into the system context.
3. `aonyx-skills.match_active(user_msg, project)` → injects active skills' prompts.
4. `aonyx-llm.chat_stream(messages, tools)` → stream chunks.
5. On tool call: `aonyx-tools.invoke(call)` → `ApprovalGate` if destructive → result.
6. Loop until model emits no tool call.
7. Post-turn: classifier inspects output → maybe `diary_append`, maybe `kg_upsert`.

### 3. Cold recall ("what did I decide about X last month?")
1. `aonyx-memory.hybrid_search(query="X", as_of="2026-04-30", k=5)`.
2. BM25 + vectors + RRF + temporal filter.
3. Top result includes diary entry + KG snapshot.
4. Agent cites source: `[diary 2026-04-23] [kg entity #142]`.

## Tradeoffs deliberately taken

| Choice | Alternative considered | Why we chose ours |
|---|---|---|
| Rust | Python | Single binary, perf, multi-OS distribution. Loss: slower iteration. |
| SQLite | Postgres / Qdrant | Zero-deps, file-portable memory palace. Loss: no shared multi-user memory in V1. |
| `fastembed-rs` (ONNX) | `candle` (full Rust ML) | ONNX is more battle-tested for embeddings. Loss: depends on ONNX runtime. |
| HNSW in-process | External vector DB | No infra. Loss: index rebuild needed on large schema migrations. |
| `rmcp` | Hand-rolled MCP | Lean on Anthropic's official client. Loss: API churn. |
| Workspace (10 crates) | Single big crate | Independent CI / publishing / boundaries. Loss: more `Cargo.toml`. |

## Non-functional requirements

- **Correctness**: every persistence path goes through a typed API + integration test with `tempfile`.
- **Observability**: `tracing` everywhere, optional JSON output for log shipping.
- **Reliability**: every external call wrapped in retry + circuit breaker.
- **Safety**: `#![forbid(unsafe_code)]` on every crate except where ONNX bindings force it.
- **Portability**: CI matrix Linux/macOS/Windows, all targets shipped.
- **Footprint**: < 25 MB stripped per binary.

---

## Open architectural questions

- Embedding model: stick with MiniLM-L6 (multilingual)? Or default to `bge-small` for English-heavy users?
- HNSW persistence: serialize whole index per project, or incremental writes?
- Should `aonyx-mcp` be split into `-client` / `-server` crates? (Maybe in V1.5 when adapters land.)

These will be resolved during V1 implementation and logged in `decisions.md`.
