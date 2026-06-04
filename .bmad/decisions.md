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

## ADR-006 — auto_retrieve: retrieve-then-generate for served deployments (2026-06-04) — accepted

**Context**: Weak/local models (e.g. an 8B via Ollama) frequently fail to call `rag_search` on interrogative messages, so the memory palace goes unused for exactly the privacy-first, local-model users who most need it.

**Decision**: Add an opt-in `auto_retrieve` path in `AgentRunner`: pre-fetch RAG context for the user's message and inject it as a source-attributed system block before the loop, on the `aonyx serve` surfaces (Telegram / Discord / OpenAI / API). Off by default; `top_k` clamped `1..=10`; minimum message-length gate; best-effort no-op on any failure. The agent remains free to call `read_document` / `find_related` (pre-load, not replace).

**Consequences**:
- ✅ The palace is used even by models that don't tool-call reliably — directly serves goal G2.
- ✅ Safe: opt-in, bounded, no-op on error; default behaviour unchanged.
- ❌ One extra retrieval per served turn when enabled; mitigated by the min-len gate + `top_k` clamp.
- ❌ Interactive TUI not covered yet (fast-follow).

---

## ADR-007 — Telegram live token streaming via editMessageText (2026-06-04) — accepted

**Context**: The Telegram adapter delivered replies as a single block; the agent loop already streams (`run_streaming` / `TurnEvent`) but it was not bridged to the adapter.

**Decision**: Add `StreamEvent {Status, Delta, Final}` to `aonyx-adapters` with a default `AgentHandler::handle_stream` (aggregate → one Final, so Discord / the OpenAI server stay unchanged), and have the Telegram adapter edit a placeholder message in place as deltas arrive (throttled ≈900 ms, edit-only-when-changed, 4096-char live cap, chunked final).

**Consequences**:
- ✅ Token-by-token UX on Telegram; other adapters unaffected (default handler).
- ✅ Tool calls surface as a status line, never raw JSON.
- ❌ `tokio` becomes a non-optional dep of `aonyx-adapters` (the streaming trait needs the mpsc type). Accepted.
- ❌ Final reply is plain text for now; CommonMark → MarkdownV2 is a follow-up.

---

*Future decisions append here.*
