# BMAD — PRD (Product Manager)

**Project**: Aonyx Agent
**Phase**: 2 — Product
**Date**: 2026-05-28
**Status**: Draft v0.1

---

## Goals

| ID | Goal |
|---|---|
| G1 | Ship a **single-binary** Rust CLI agent (`aonyx`) that runs autonomously, multi-turn, with a built-in tool registry. |
| G2 | Provide a **memory palace** out of the box: structured KG, diary, hybrid search, cross-link, time-machine. |
| G3 | Be **multi-provider** by design: Anthropic, OpenAI, OpenRouter, Ollama, LM Studio, Nous Portal — swappable in one command. |
| G4 | Auto-generate **skills** (SKILL.md format, agentskills.io-compatible) after recurring task shapes (≥3 occurrences). |
| G5 | Be **installable** in <60 seconds via `cargo install` / `brew` / `winget` / direct binary. |

## Scope — Vague 1 (MVP)

### In scope
- **CLI** (`aonyx`, `aonyx new <path>`, `aonyx resume`, `aonyx config`, `aonyx memory <subcmd>`, `aonyx skills <subcmd>`)
- **Agent loop**: streaming, tool dispatch, context compression, recent-call cycle detection, max-iter guard
- **Approval gate**: classify each tool call (`safe` / `caution` / `destructive`) with policy file
- **Memory palace** (SQLite, bundled):
  - Entities + relations with `valid_from` / `valid_to` (port of Aonyx RAG `kg/store.py`)
  - Diary append-only per project (port of `agent/diary.py`)
  - Hybrid search: BM25 + vectors + RRF (k=60) with temporal boost (port of `utils/hybrid_search.py`)
  - Tree-sitter AST splitter for code (port of `utils/code_splitter.py`) — Python, JS, TS, Rust, Go
  - Cross-project linking via centroid cosine (port of `cross_linking.py`)
  - `as_of` queries on every search endpoint
- **LLM providers** (5 in V1): Anthropic, OpenAI, OpenRouter, Ollama, LM Studio (OpenAI-compatible custom)
- **Built-in tools**: `fs_read`, `fs_write`, `fs_edit`, `fs_glob`, `fs_grep`, `bash`, `git_status`, `git_diff`, `git_log`, `git_show`, `exec`, `web_fetch`, `web_search` (Brave/Tavily), `memory_search`, `memory_kg_query`, `memory_diary_append`
- **Skills**:
  - `SKILL.md` loader (frontmatter YAML + body)
  - 4 built-in skills (port the Aonyx RAG YAML: `code-review`, `data-analyst`, `doc-writer`, `incident-response`)
  - Trigger matching: keywords, regex, project pattern, manual, always-on
- **MCP**: client (consume external servers) + server (expose Aonyx tools)
- **Config**: `~/.aonyx/config.toml` + API keys in OS keyring (`keyring` crate)
- **Persistence**: `~/.aonyx/sessions.db` (FTS5) + per-project `./.aonyx/` (KG, diary)
- **SOUL.md** global + `agent.yaml` per project
- **Distribution**: GitHub Releases (static binaries Linux x64/arm64, macOS x64/arm64, Windows x64) + `cargo install aonyx-agent`

### Out of scope (Vague 1)
- TUI (`ratatui` crate scaffolded but lib only; full UI in V1.5)
- Messaging adapters (Telegram, Discord) → Vague 2
- OpenAI-compatible HTTP server → Vague 2
- Browser automation, vision, image gen, TTS → Vague 3
- Self-evolution (DSPy/GEPA-style) → Vague 3
- Modal / Daytona / Singularity backends → Vague 3
- Subagent spawning with isolation → Vague 2

## Non-goals

- We will **not** ship a hosted SaaS in V1.
- We will **not** support 20+ messaging platforms (Hermes's territory).
- We will **not** bundle proprietary models or keys.
- We will **not** require a cloud account to run.

## Success metrics (V1 release)

| ID | Metric | Target |
|---|---|---|
| M1 | First-token latency (Anthropic cloud, p50) | < 2 s |
| M2 | First-token latency (Ollama llama3.1:8b local, p50) | < 4 s |
| M3 | Memory palace ingest throughput | ≥ 100 chunks/sec on commodity laptop |
| M4 | Hybrid search recall@10 on the eval set | ≥ 90 % |
| M5 | Cold install to first reply | < 60 s on warm network |
| M6 | Binary size (release, stripped) | < 25 MB per arch |
| M7 | Crash rate over 1 h soak test | 0 |

## Risk register

| ID | Risk | Mitigation |
|---|---|---|
| R1 | Rust ecosystem for local embeddings is younger than Python's | Pin `fastembed-rs` + fallback to remote embedding endpoint |
| R2 | Tree-sitter Windows builds can be brittle | Use prebuilt `tree-sitter-{lang}` crates only; CI matrix covers Win |
| R3 | MCP `rmcp` is fresh, breaking changes possible | Vendor minimal subset; isolate behind `aonyx-mcp` crate |
| R4 | Skill auto-generation needs a strong model | Make threshold configurable; default off until V1.2 |
| R5 | Confusion with Aonyx RAG branding | Tagline + docs make distinction explicit; separate repo and identity |

## Hypotheses to validate during V1

- **H1**: A memory palace differentiator is worth the engineering cost vs a "good enough" `MEMORY.md` (Hermes parity). Validated by: user interviews + GitHub star velocity vs comparable agents.
- **H2**: Rust single-binary is meaningful enough for adoption that it offsets the loss of Python's `pip install plugin` ergonomics. Validated by: install-success surveys + plugin contribution rate.
- **H3**: Multi-provider out-of-box matters more than first-class Anthropic-only. Validated by: telemetry opt-in on provider distribution.

## Vague 2 (post-MVP, indicative)

- TUI (`ratatui`) with streaming, slash commands, OSC-52 clipboard, status bar
- Adapters: Telegram (`teloxide`), Discord (`serenity`)
- OpenAI-compatible HTTP server (`/v1/chat/completions`, `/v1/responses`)
- Subagent spawning with isolation
- Plugin system (Lua via `mlua`, or WASM via `wasmtime`)
- Skill auto-generation enabled by default

## Vague 3 (long-term, indicative)

- Browser automation (`fantoccini` or CDP via `chromiumoxide`)
- Vision (multimodal models via providers)
- Image gen / TTS providers
- Self-evolution loop (Rust port of DSPy/GEPA concepts)
- Cloud sync (encrypted memory palace backup)
- Modal / Daytona / Singularity terminal backends

---

## Acceptance criteria (Vague 1)

- [ ] `cargo install --path crates/aonyx-cli` then `aonyx` opens an interactive session.
- [ ] An out-of-the-box conversation produces at least one diary entry and one KG entity after a non-trivial multi-turn task.
- [ ] `aonyx memory search "<query>"` returns hybrid-ranked results with sources.
- [ ] `aonyx config set provider anthropic` then any prompt is served via Anthropic; switching to `ollama` works without restart.
- [ ] All 4 built-in skills load and trigger correctly on sample prompts.
- [ ] The MCP server (`aonyx mcp serve`) exposes 10+ tools consumable from Claude Code.
- [ ] CI green on Linux / macOS / Windows.
- [ ] A first GitHub Release with three signed static binaries.
