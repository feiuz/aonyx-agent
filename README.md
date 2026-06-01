# 🦦 Aonyx Agent

> **The agent with a real memory palace.**
> An open-source, memory-first AI agent: Knowledge Graph + Hybrid Search + Time-machine. Single binary, multi-provider LLM, MIT licensed.

[![CI](https://github.com/feiuz/aonyx-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/feiuz/aonyx-agent/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](rust-toolchain.toml)

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

**Pre-alpha — scaffolding phase.** The workspace is laid out, BMAD artefacts are in `.bmad/`, but most crates are stubs. See [`.bmad/prd.md`](.bmad/prd.md) for the MVP scope and [`CHANGELOG.md`](CHANGELOG.md) for progress.

---

## Quickstart

```bash
# Install from crates.io (installs the `aonyx` binary)
cargo install aonyx-agent
# or grab a prebuilt static binary from the Releases page:
#   https://github.com/feiuz/aonyx-agent/releases/latest

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
```

---

## Architecture

Cargo workspace, 10 crates:

```
aonyx-core        Shared types, traits, errors
aonyx-memory      ⭐ Memory palace: KG + diary + hybrid search + time-machine
aonyx-llm         Provider router: Anthropic, OpenAI, OpenRouter, Ollama, LM Studio, Nous Portal
aonyx-tools       Built-in tools: fs, bash, git, exec, web, memory_*
aonyx-skills      SKILL.md engine + loader + auto-generation
aonyx-agent       Agent loop, compaction, classifier, subagents, approval gate
aonyx-mcp         MCP client (consume servers) + MCP server (expose self)
aonyx-cli         The `aonyx` binary (clap)
aonyx-tui         Interactive TUI (ratatui) — Wave 1.5
aonyx-adapters    Telegram / Discord / OpenAI-compatible HTTP — Wave 2
```

Full design rationale in [`.bmad/architecture.md`](.bmad/architecture.md).

---

## Roadmap

See [`.bmad/prd.md`](.bmad/prd.md) for the full Vague 1 / Vague 2 / Vague 3 plan. Highlights:

- **Vague 1 (MVP)** — CLI, memory palace, 5 LLM providers, fs/bash/git/exec/web tools, 4 built-in skills, MCP client+server.
- **Vague 2** — TUI, Telegram + Discord adapters, OpenAI-compatible server, subagents.
- **Vague 3** — Browser automation, vision, image gen, TTS, self-evolution (DSPy/GEPA-style), Modal/Daytona backends.

---

## License

MIT — see [`LICENSE`](LICENSE).
