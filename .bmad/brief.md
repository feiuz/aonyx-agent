# BMAD — Brief (Analyst)

**Project**: Aonyx Agent
**Phase**: 1 — Discovery
**Date**: 2026-05-28
**Method**: BMAD (Breakthrough Method of Agile AI-driven Development)

---

## Vision (one line)

> *The only AI agent with a real memory palace — Knowledge Graph + Hybrid Search + Time-machine — open source, single binary, runs on any LLM.*

## Personas

- **P1 — Power user dev / techie**
  Uses Claude Code, Cursor, Aider. Wants a local-first agent without cloud lock-in. Cares about privacy and provider neutrality.
- **P2 — Knowledge worker**
  Journalist, researcher, consultant. Needs a *second brain* that remembers ongoing dossiers, sources, decisions — not just chats.
- **P3 — Creator solo / hobbyist**
  Indie dev, content creator, small team. Wants the multi-channel and skill-loop magic of Hermes, with stronger memory.

## Problem

Existing agents store memory in flat files (Hermes' `MEMORY.md`, Claude Code's `CLAUDE.md`) or proprietary clouds (ChatGPT memory). None of them:

- Maintain a **temporal** knowledge graph (when did this fact become true? is it still true?).
- Search **hybrid** (lexical BM25 + semantic vectors + RRF) for high-recall recall on technical identifiers.
- Cross-link **across projects** so the agent can recognize that today's research connects to last month's project.
- Split **code** by AST so a recalled chunk is always a coherent function or class.
- Travel back in **time** (`as_of`) to reconstruct what the agent knew at a given date.

These memory patterns already exist battle-tested in the Aonyx RAG codebase (v3.0.0 MemPalace integration). Aonyx Agent ports them to a single Rust binary, packaged for the world.

## Inspiration

- **Hermes Agent** (Nous Research, MIT) — the agent-loop / skill auto-generation / multi-channel patterns.
- **Aonyx RAG** (proprietary backend) — the memory palace patterns (KG, hybrid search, code-aware splitter, diary, cross-linking, time-machine).
- **Claude Code, Aider, Cursor** — the dev-first UX bar.

## Why now

- Aonyx RAG just shipped v3.0.0 (May 2026) consolidating the memory architecture in production over 25 projects / 736 chunks. The patterns are validated.
- The agent-OS race (Hermes, Open Interpreter, smolagents, agno) is heating up, but **none** of them has a structured memory layer comparable to a KG + hybrid + time-machine.
- Rust async ecosystem is mature enough (tokio, rmcp, fastembed-rs, ratatui) to deliver a single-binary cross-platform agent.

## Positioning vs Hermes Agent

| Axis | Hermes Agent | Aonyx Agent |
|---|---|---|
| Slogan | *The agent that grows with you* | *The agent with a real memory palace* |
| Differentiator | 20+ messaging adapters, self-evolving skills | **Memory architecture** (KG, hybrid, time-machine, code-aware) |
| Language | Python | **Rust** (single binary) |
| Memory | MEMORY.md + FTS5 + Honcho | **SQLite KG** + BM25 + vectors + RRF + time-machine |
| Distribution | pip / installer scripts | `cargo install` / `brew` / `winget` / static binary |
| License | MIT | MIT |

Aonyx Agent does **not** compete on adapter breadth (Hermes is unbeatable there in V1). It competes on **memory depth**, **single-binary ergonomics**, and **Rust-grade reliability**.

## Success criteria (12 months)

1. 1 000 GitHub stars on `aonyx-agent`.
2. 10+ third-party skills published on agentskills.io.
3. p50 first-token latency < 2 s on the default cloud config.
4. Memory palace exports / imports with zero data loss.
5. Featured in at least 2 agent-OS comparative reviews alongside Hermes.

## Open questions for PM phase

- Default model on first run: cloud (requires key) or local (Ollama auto-download)?
- Memory storage location: project-local (`./.aonyx/`) or user-global (`~/.aonyx/`)? Or both?
- Skills format: strict agentskills.io compat, or extend it (we'd contribute upstream)?
