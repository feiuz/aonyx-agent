# Changelog

All notable changes to **Aonyx Agent** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-05-28 — pre-alpha foundations

This is the first release. Aonyx Agent runs end-to-end against Anthropic / OpenAI /
OpenRouter / Ollama / LM Studio with a working memory palace (Knowledge Graph,
diary, BM25 full-text search) and four built-in skills.

### Added

#### Agent core
- `aonyx-agent::AgentRunner` — multi-turn loop with streaming, tool dispatch,
  `ApprovalPolicy` gate, per-turn iteration cap, skill activation, project context.
- `ApprovalPolicy` with `AutoAllow`, `DenyDestructive` (default), `Custom(Arc<Fn>)`.
- `ChatRequest` / `ChatChunk` / `ChatStream = BoxStream<'static, Result<ChatChunk>>`
  shared types in `aonyx-core`.

#### Memory palace
- `aonyx-memory::SqliteKgStore` — Knowledge Graph with entity / relation
  temporal validity windows, idempotent migrations, indexes, 5 tests.
- `aonyx-memory::SqliteDiaryStore` — append-only narrative log per project.
- `aonyx-memory::SqliteChunksStore` — SQLite FTS5 BM25 search with
  `unicode61 remove_diacritics 2` tokenizer.
- `aonyx-memory::Palace` — unified facade composing the three stores; `open(dir)`
  creates `{kg.db, diary.db, chunks.db}` layout under `./.aonyx/`.
- `MemoryStore::hybrid_search` delegates to BM25 (FTS5). Vector layer
  (fastembed-rs + HNSW + RRF k=60) intentionally deferred to V1.1.

#### LLM providers
- `aonyx-llm::anthropic::AnthropicProvider` — native Messages API,
  streaming SSE, `content_block_delta` + `message_stop` events,
  system-message extraction.
- `aonyx-llm::openai_compat::OpenAiCompatProvider` — shared backend for every
  "speaks-OpenAI" endpoint. Optional Bearer auth, optional extra headers.
- `aonyx-llm::openai::provider` — OpenAI public API factory.
- `aonyx-llm::openrouter::provider` + `provider_with_attribution` —
  OpenRouter aggregator with optional `HTTP-Referer` / `X-Title`.
- `aonyx-llm::lm_studio::provider` — LM Studio with empty Bearer (no auth header).
- `aonyx-llm::OllamaProvider` — JSON-lines streaming from `/api/chat`.
- `aonyx-llm::Router` — fallback chain across providers with `tracing::warn` on each failure.

#### Tools (10 built-in handlers, registered by `ToolRegistry::default_set()`)
- `fs_read`, `fs_glob`, `fs_grep` — `Safe`.
- `fs_write`, `fs_edit` — `Destructive` (must clear `ApprovalPolicy`).
- `bash` — `Destructive`; `cmd /C` on Windows, `sh -c` elsewhere; timeout via
  `tokio::time::timeout`; `kill_on_drop`.
- `git_status`, `git_diff`, `git_log`, `git_show` — `Safe`.

#### Skills
- `aonyx-skills::SkillLoader` — parses YAML frontmatter + markdown body from
  any `SKILL.md` / `*.skill.md`. Handles `\n` and `\r\n` line endings.
- `aonyx-skills::SkillEngine` — activates skills via case-insensitive keywords,
  query regex, project regex, `always_on`, or `manual`. Invalid regexes are
  silently skipped.
- `aonyx-skills::builtin_skills()` returns the four V1 built-ins embedded
  in the binary at compile time:
  `code-review`, `doc-writer`, `data-analyst`, `incident-response`.

#### CLI
- `aonyx` — opens an interactive REPL in the current dir.
- `aonyx new <path>` — same, scoped to `<path>`.
- `aonyx config show / path` — inspect `~/.aonyx/config.toml`.
- `aonyx memory stats` — report kg / diary / chunk counts.
- `aonyx memory search <query>` — BM25 search across chunks.
- Slash commands inside a session: `/quit /q /exit`, `/clear /reset`, `/help /?`.
- First-run wizard writes `~/.aonyx/config.toml` with sensible defaults.
- Environment fallbacks: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
  `OPENROUTER_API_KEY`.

#### Distribution
- MIT license, public repository at https://github.com/feiuz/aonyx-agent.
- CI matrix on Linux, macOS, Windows for format / clippy / test.
- Release pipeline (`.github/workflows/release.yml`) triggers on `v*.*.*`
  tags and produces static binaries for Linux x86_64, macOS x86_64 + arm64,
  Windows x86_64.
- `release.toml` for `cargo-release` automation; `docs/releasing.md` walkthrough.

### Numbers
- 90 tests across 5 crates (8 agent + 26 llm + 22 memory + 14 tools + 13 skills + 7 cli).
- `aonyx.exe` release binary: 8.0 MB stripped.
- p50 cold start to interactive prompt: well under 1 s.

### Known gaps (planned for V1.1+)
- Vector embeddings (`fastembed-rs` ONNX), HNSW index, RRF fusion, temporal boost.
- Tree-sitter code-aware chunk splitter.
- MCP client and server (`aonyx-mcp` crate is scaffolded but inert).
- Interactive approval prompt (CLI currently only supports `DenyDestructive`).
- Subagent spawning (`aonyx-agent::subagent` is scaffolded but inert).
- Telegram / Discord adapters (`aonyx-adapters` is scaffolded but inert).
- OpenAI-compatible HTTP server.
- `tools` blocks in OpenAI / Ollama provider payloads (text-only V1).

## [Unreleased]

_(no changes yet)_
