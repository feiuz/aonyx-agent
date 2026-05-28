# BMAD — Decision Log (ADRs)

Architectural decisions for Aonyx Agent. New decisions are **appended only**.

Format: `ADR-NNN — short title (date) — status`

---

## ADR-001 — Use Rust as the implementation language (2026-05-28) — accepted

**Context**: Aonyx Agent must ship as a single binary, run on Linux/macOS/Windows, and feel like an installed tool, not a Python environment.

**Decision**: Implement the agent in Rust (stable channel, edition 2021).

**Consequences**:
- ✅ Single static binary per OS/arch, trivial install (`cargo install` / `brew` / `winget`).
- ✅ Strong type system catches whole classes of agent-loop bugs at compile time.
- ✅ `tokio` + `tracing` give first-class async observability.
- ❌ Existing Aonyx RAG Python code cannot be reused as-is; we **port patterns**, not code.
- ❌ Slower iteration than Python (compile times); offset by `cargo check` + workspace caching.

---

## ADR-002 — Cargo workspace with 10 crates (2026-05-28) — accepted

**Context**: We want clean module boundaries, independent testing, and the option to publish individual crates on crates.io.

**Decision**: One workspace, ten crates, listed in `Cargo.toml` members.

**Consequences**:
- ✅ Each crate has its own `Cargo.toml` and can be versioned independently.
- ✅ Boundary violations become compile errors.
- ❌ More files at the root. Standard Rust ecosystem pattern, accepted.

---

## ADR-003 — Memory storage: SQLite per project + global session DB (2026-05-28) — accepted

**Context**: Hermes uses a single SQLite for sessions. Aonyx wants per-project memory palaces so a user can hand a project folder to a collaborator and the palace travels with it.

**Decision**:
- `~/.aonyx/sessions.db` for cross-project session history and FTS5.
- `./.aonyx/palace.db` per project for KG, diary, chunks, embeddings.

**Consequences**:
- ✅ Project-portable memory.
- ✅ User can `git ignore` `./.aonyx/` or commit it (their choice).
- ❌ Cross-project search must aggregate across multiple DBs. Manageable via `aonyx-memory` orchestration.

---

## ADR-004 — Multi-provider LLM by default (2026-05-28) — accepted

**Context**: Vendor lock-in is the #1 complaint with first-gen agents. Hermes ships 30+ providers; we keep this strength.

**Decision**: Five providers in V1 (Anthropic, OpenAI, OpenRouter, Ollama, LM Studio). Provider chain with fallback configured per session.

**Consequences**:
- ✅ Any user, any budget, any privacy stance.
- ❌ More surface to maintain; mitigated by all OpenAI-compat providers sharing one implementation.

---

## ADR-005 — Embedding stack: fastembed-rs (ONNX) + hnsw_rs (2026-05-28) — accepted

**Context**: We need a Rust-native local embedding solution. Options: `candle`, `fastembed-rs`, remote-only.

**Decision**: `fastembed-rs` (default MiniLM-L6 multilingual) with `hnsw_rs` for ANN search. Remote embedding endpoint as fallback.

**Consequences**:
- ✅ Zero-config local embeddings, no Python.
- ❌ Adds ONNX runtime binary to the bundle (~20 MB). Accepted for the memory-palace differentiator.

---

*Future decisions append here.*
