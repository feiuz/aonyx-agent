# 🦦 Aonyx Agent

> **The agent with a real memory palace.**
> An open-source, memory-first AI agent: Knowledge Graph + Hybrid Search + Time-machine. Single binary, multi-provider LLM, MIT licensed.

[![CI](https://github.com/feiuz/aonyx-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/feiuz/aonyx-agent/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96+-orange.svg)](rust-toolchain.toml)
[![crates.io](https://img.shields.io/crates/v/aonyx-agent.svg)](https://crates.io/crates/aonyx-agent)

---

## Why another agent?

Most agents remember in flat files (`MEMORY.md`, plain notes). Aonyx Agent treats memory as a **first-class structured palace**:

| Capability | Most agents | Aonyx Agent |
|---|---|---|
| Persistence | Flat markdown | SQLite **Knowledge Graph** (entities/relations) |
| Search | Full-text only | **Hybrid**: BM25 + vectors + RRF fusion |
| Time | "Now" only | **Time-machine** queries (`as_of`), validity windows |
| Code | Generic chunks | **Tree-sitter** AST-aware splitting |
| Across projects | Isolated | **Cross-linking** sémantique inter-projets |
| User model | Token concat | Preferences + diary, per project |

Inspired by [Hermes Agent](https://github.com/NousResearch/hermes-agent) (Nous Research) for the multi-channel/skill-loop patterns, and by Aonyx RAG (a private memory system) for the memory architecture.

---

## Status

**v0.5.0 — released.** Vague 1 (MVP) and Vague 2 are complete; Vague 3 is in progress (vision, browser automation, image-gen, TTS). Published on crates.io (`cargo install aonyx-agent`); prebuilt binaries — **lean** and **-full** (all chat adapters + Lua plugins + browser automation compiled in) — on the [Releases](https://github.com/feiuz/aonyx-agent/releases/latest) page. `clippy --all-features -D warnings` clean on a pinned 1.96.0 toolchain; full workspace test suite green. See [`CHANGELOG.md`](CHANGELOG.md) for per-release detail and [`.bmad/prd.md`](.bmad/prd.md) for the roadmap.

> API keys are stored in the OS keyring via `aonyx setup` (resolution order: `config.toml` → keyring → env var). Prebuilt binaries cover Linux x86_64 + aarch64, macOS x86_64 + arm64, and Windows x86_64. Grab the **`-full`** archive for the Telegram/Discord/OpenAI-server adapters + Lua plugins + browser automation, or build them in with `cargo install aonyx-agent --features telegram,discord,openai-server,lua-plugins,browser`.

---

## Quickstart

```bash
# Install from crates.io (installs the `aonyx` binary)
cargo install aonyx-agent
# or grab a prebuilt static binary from the Releases page:
#   https://github.com/feiuz/aonyx-agent/releases/latest

# One-time: pick a provider, store the key in your OS keyring, test it
aonyx setup

# First run — interactive session in the current directory
aonyx
aonyx --tui                       # full-screen terminal UI

# New session scoped to a project
aonyx new ./my-research

# Resume the last session
aonyx resume

# Inspect your memory palace
aonyx memory stats
aonyx memory search "decisions about auth"

# Run it as a chat bot (install once with the matching feature):
#   cargo install aonyx-agent --features telegram   # and/or --features discord
aonyx setup telegram              # store the bot token (keyring) + allowed chats
aonyx serve telegram              # bridge Telegram to the agent loop
aonyx setup discord && aonyx serve discord   # …or Discord

# …or expose an OpenAI-compatible HTTP API (install with --features openai-server):
aonyx serve openai --port 8787    # POST /v1/chat/completions for any OpenAI SDK

# …or extend the agent with a Lua tool (install with --features lua-plugins):
cp examples/plugins/hello.lua ~/.aonyx/plugins/   # the agent gains a `hello` tool
```

---

## Architecture

Cargo workspace, 9 crates:

```
aonyx-core        Shared types, traits, errors
aonyx-memory      ⭐ Memory palace: KG + diary + hybrid search (BM25 + fastembed vectors + RRF) + tree-sitter splitter + cross-linking + time-machine
aonyx-llm         Provider router: Anthropic, OpenAI, OpenRouter, Ollama, LM Studio, Claude Code
aonyx-tools       Built-in tools: fs, bash, git, web_fetch, web_search, memory_*, image_gen, tts + Lua plugin loader + browser automation (feature-gated)
aonyx-skills      SKILL.md engine + loader + 4 built-in skills + trigger matching + auto-generation
aonyx-agent       The `aonyx` binary (clap CLI + ratatui TUI) AND the agent-loop library (loop, compaction, classifier, subagents, approval gate)
aonyx-mcp         MCP client (stdio + HTTP) + MCP server (expose self)
aonyx-adapters    Channel adapters (feature-gated): Telegram (teloxide) + Discord (serenity) bots + OpenAI-compatible HTTP server (axum)
aonyx-tui         Reserved placeholder (the live TUI ships inside aonyx-agent)
```

Full design rationale in [`.bmad/architecture.md`](.bmad/architecture.md).

---

## Roadmap

See [`.bmad/prd.md`](.bmad/prd.md) for the full plan. Where we are:

- **Vague 1 (MVP)** — ✅ done: CLI, memory palace (KG + hybrid search + tree-sitter + cross-linking + time-machine), 6 LLM providers, fs/bash/git/web tools, 4 built-in skills, MCP client + server.
- **Vague 2** — ✅ complete: full ratatui TUI, subagents, MCP client + server, **Telegram** + **Discord** + **OpenAI-compatible HTTP server** + **Lua plugins** (feature-gated), and **skill auto-generation** (on by default).
- **Vague 3** — in progress: vision ✅ (Anthropic + OpenAI passthrough) + **browser automation** ✅ (`chromiumoxide` headless Chrome, feature-gated) + **image-gen** ✅ + **TTS** ✅ (OpenAI images/audio). Next: cloud sync, self-evolution.

---

## License

MIT — see [`LICENSE`](LICENSE).
