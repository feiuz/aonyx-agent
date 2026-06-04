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

## ADR-008 — RAG backend selectable at setup: local palace vs external MCP (2026-06-04) — accepted

**Context**: Retrieval can come from the built-in memory palace (local, offline, zero-server) or from an external RAG via an MCP `__rag_search` tool (e.g. the user's own `aonyx-rag`). Forcing one path hurts either the privacy-first local user or the power user with a central multi-project palace. `auto_retrieve` (ADR-006) already targets whatever `rag_search` tool is present.

**Decision**: Make the backend a setup choice — `[rag] backend = "local" | "external"`. `local` registers a built-in `rag_search` tool backed by the palace; `external` relies on the configured MCP `__rag_search`. Both return the same JSON shape (`{project, source, content, score}`) so `auto_retrieve` and tool-calling are backend-agnostic. Plan: [`rag-in-app-prd.md`](rag-in-app-prd.md).

**Consequences**:
- ✅ Self-contained offline RAG for the OSS user **and** the central-palace path for power users — no fork in the agent loop.
- ✅ Clean-room (ADR-001): the external path is the user's own server via MCP; the local path never pipes to it nor ingests its data.
- ❌ Two backends to keep contract-compatible; mitigated by a shared result shape + tests.

---

## ADR-009 — Embeddings selectable at setup: local vs provider (2026-06-04) — accepted

**Context**: Extends ADR-005 (fastembed-rs + hnsw_rs, remote fallback). Embeddings ≠ chat: **Anthropic and claude-code expose no embeddings API**, so "use the configured LLM" cannot supply vectors for those users. Embeddings must be decoupled from the chat provider.

**Decision**: `[rag] embeddings = "local" | "provider"`, chosen at setup. `local` (**default**) = fastembed-rs ONNX, multilingual, downloaded on first use (no bundle, no Python, offline). `provider` = an embeddings HTTP endpoint (OpenAI / Ollama) reusing the provider config. Store `model_id` + `dim` per vector; a change triggers re-index. Plan: [`rag-in-app-prd.md`](rag-in-app-prd.md).

**Consequences**:
- ✅ RAG works regardless of the chat provider (incl. Anthropic / claude-code) — the local default is always available, offline.
- ✅ Higher-quality cloud embeddings available opt-in for users who already pay for them.
- ❌ Dimension/model coherence must be tracked; mitigated by `model_id` + `dim` columns + `aonyx memory reindex`.
- ❌ First local use downloads a 30–130 MB model; mitigated by lazy download + a clear notice (keeps the binary lean, M6).

---

*Future decisions append here.*
