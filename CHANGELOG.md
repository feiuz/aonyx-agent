# Changelog

All notable changes to **Aonyx Agent** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — the connected agent

The post-0.2.0 arc (phases AA → RR) opens Aonyx up to the wider tool
ecosystem and deepens the memory-palace integration. `clippy --workspace
--all-targets --all-features -D warnings` clean on a pinned 1.96.0
toolchain (local == CI); ~280 workspace tests.

### Added

#### MCP (Model Context Protocol)
- **MCP client** — connect to remote MCP servers over **stdio** (GG) and
  **Streamable HTTP/SSE** (II); their tools register into the catalogue as
  `<server>__<tool>` and are callable like any built-in.
- **MCP server** — `aonyx mcp serve` exposes Aonyx's own tools to other
  clients (Claude Code, Cursor, …) over stdio (HH). It now also serves the
  palace-backed `memory_*` tools scoped to the current directory (**NN**),
  and over a minimal **Streamable HTTP** transport via `--port` (**OO**),
  with optional **bearer-token auth** (`--token` / `$AONYX_MCP_TOKEN`)
  rejecting unauthorized HTTP requests with `401` (**PP**).

#### Built-in tools
- `web_fetch` (readability extraction) and `web_search` (Brave → Tavily
  fallback) (JJ, MM); `web_fetch` now extracts text from **PDFs** too
  (**PP**).
- `memory_search` / `memory_diary_append` / `memory_kg_query` — the agent
  reads and writes its own memory palace mid-turn (MM).

#### Sessions & providers
- `/fork` a session into a child branch (Z); auto-compact long sessions
  (BB); `/retry` the last turn (CC); `/tree` session genealogy (MM).
- `/model` and `/provider` live-switch (EE, LL); `/provider` persists the
  choice and **remaps the model to the new provider's default** when the
  active id doesn't fit (**NN**).

#### Vision & export
- Local `@image` references are downscaled to ≤1568px before being sent to
  a vision model, capping token cost (**NN**). Remote image **URLs** are
  fetched (and downscaled) into vision attachments too (**OO**).
- `/export-html` standalone styled HTML (FF); `/export-bundle` writes a
  `.zip` of Markdown + HTML + `meta.json` (**NN**), plus a `messages.json`
  transcript for re-import (**OO**). `/import-bundle <zip>` restores a
  session from that `messages.json` as a fresh, active session (**PP**).

#### Approval
- Per-tool **always-allow**: the approval overlay's `[A]` key remembers a
  destructive tool so future calls skip the prompt; persisted to config
  (**OO**). Rules also accept a `name:needle` arg-pattern form — e.g.
  `bash:cargo` auto-approves only cargo commands (**PP**).

#### Robustness & cost
- HTTP providers (Anthropic / OpenAI-compatible / Ollama) **retry**
  transient 429 / 5xx / network errors with exponential backoff (**RR**).
- Anthropic **prompt-caching**: the system prompt is sent as a cached
  block, cutting input-token cost across a session's turns (**RR**).
- `/cost` prints a detailed per-session token + USD breakdown (**RR**).

#### TUI
- `/mcp` panel lists connected MCP servers and toggles all of a server's
  tools at once (**RR**). `/rename` retitles the current session (**RR**).
  `@glob` refs (`@src/**/*.rs`) load every matching file (**RR**).

#### Skills & theming
- Custom skills loaded from `~/.aonyx/skills/` (DD); live theme editor
  `/theme-edit` (KK).

## [0.2.0] — 2026-05-29 — the full-screen TUI

A 25-phase arc (B → Z) turning the line-based REPL into a full-screen
`ratatui` terminal UI with multimodal input, multi-session branching, a
memory-palace inspector, and live capability toggles. 185 workspace tests,
`clippy --workspace -D warnings` clean.

Launch with `aonyx --tui`. The legacy line REPL remains the default.

### Added

#### Terminal UI (ratatui)
- Full-screen layout: scrollable conversation viewport, multi-line composer,
  status bar. Auto-scroll, animated braille spinner, `💭 thinking…`
  placeholder.
- Markdown rendering in the viewport via `tui-markdown`, with a
  `ratatui_core → ratatui` colour converter. Rendered **live during
  streaming** — headings / bold / code light up as the model types.
- `@path` file references load files into the next turn's context;
  `!cmd` runs a local shell and feeds the output back.
- Fuzzy autocomplete popup (`nucleo-matcher`) for `@` files, `/` commands,
  and `/cmd <arg>` argument completion (`/themes`, `/load`, `/ingest`,
  `/undo`).
- Inline composer syntax highlight: `/cmd` magenta, `!bash` yellow, chat
  default — recoloured on every keystroke.
- `Ctrl+P` fuzzy command palette over every slash command + theme.
- Mouse support: scroll wheel, single-click palette accept. `/mouse`
  toggles capture so the host terminal can do native drag-to-select + copy.
- Vim modal editing (`/vim`): Insert / Normal, `j/k/g/G` navigation.
- 4 bundled themes (`/themes`): default, catppuccin, dracula, gruvbox.
- Desktop notifications (`notify-rust`) on long-turn completion + errors.
- Token + USD cost estimator in the status bar (per-provider pricing table).

#### Multi-session
- `SqliteSessionStore` cross-run persistence at `~/.aonyx/sessions.db`,
  auto-restore on startup.
- `/sessions` list, `/new` rotate, `/find <query>` full-text search across
  every session, `/load <id-prefix>` switch, `/fork` branch the current
  session into a child (parent_id tracked).

#### Floating panels
- `/kg` — memory-palace visualization: entities grouped by type, relations
  as `src ──predicate──▶ dst`.
- `/tools` — enable / disable registered tools live (shared `ToolRegistry`
  disabled-set).
- `/skills` — enable / disable loaded skills live (shared runner toggle set).
- `/inspect` — pretty-printed JSON of the last LLM request (base64 images
  elided).

#### Safety + memory
- Inline approval overlay: destructive `fs_edit` / `fs_write` / `bash` calls
  pause the runner for a `[Y/n]` decision (async `AsyncApprover` bridge).
- `/undo [N|list]` — revert the last N filesystem changes via a JSONL
  snapshot journal at `<cwd>/.aonyx/undo.jsonl`.
- `/ingest <path>` — chunk a local file (paragraph-aligned) into the project
  palace; searchable by the agent.

#### Multimodal
- `@image.png` rendered inline as a half-block Unicode thumbnail (works in
  any truecolor terminal, no Kitty / iTerm / Sixel dependency).
- Vision passthrough: images forwarded to Anthropic (`image` / `source`
  blocks) and OpenAI-compatible providers (`image_url` data URLs).
- `Attachment::Image` on `Message`, `#[serde(default)]` for backwards
  compatibility with existing persisted rows.

### Changed
- `ApprovalPolicy::allow` is now `async` to support the interactive TUI
  approver.
- `AgentRunner` exposes shared handles: `skill_toggle_handle()`,
  `last_request_handle()`.
- `SessionStore` gains `search`, `find_by_id_prefix`, and `fork`.
- `ToolRegistry` gains a shared disabled-set with `disable` / `enable` /
  `toggle` / `is_disabled`.

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
