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

## ADR-010 — Desktop frontend: stay vanilla JS + golden-layout for docking (2026-06-04) — superseded by ADR-014

**Context**: Desktop v2 wants Claude-Code-like movable panels (drag/stack/resize, persisted), plus a conversation-history sidebar, a user-session widget, an updates zone, i18n and auth screens. The current frontend is plain vanilla JS (`desktop/src/{index.html,app.js,styles.css}`). Options: migrate to React+dockview (best docking, full rewrite), Svelte+golden-layout, or vanilla+golden-layout, or vanilla+split.js (resize only). Plan: [`desktop-v2-plan.md`](desktop-v2-plan.md).

**Decision**: Keep the **vanilla JS** frontend and add **golden-layout** (framework-agnostic docking lib) for true drag-dock with persisted layout. No framework migration. A small hand-rolled state module keeps auth/history/i18n/widget state manageable.

**Consequences**:
- ✅ Real "movable like Claude Code" docking without rewriting the working app; moderate churn, existing code preserved.
- ✅ Smallest bundle / simplest Tauri integration.
- ❌ App state stays in vanilla — discipline required (a tiny store) as auth/history/i18n grow; revisit a framework only if it gets unwieldy.

---

## ADR-011 — Desktop auth: device-code grant, optional / offline-first (2026-06-04) — accepted

**Context**: The desktop must depend on aonyx-account (account, license, sync) but the agent is local-first/offline (embedded `aonyx serve api`, local palace/RAG). aonyx-account already ships a device-code auth flow (`/api/v1/auth/device/{code,token,approve,deny}`) — the OAuth grant designed for desktop/CLI clients.

**Decision**: Authenticate via the **device-code grant** (open browser → user approves on web, where MFA/WebAuthn already live → desktop polls for JWT). Tokens stored in the **OS keyring** (Rust `keyring` crate), refreshed via `/api/v1/auth/refresh`, all behind Tauri Rust commands (`account_*`, server-to-server → no CORS). Auth is **optional and non-blocking**: the local agent works fully offline; signing in unlocks license/sync/preferences/language.

**Consequences**:
- ✅ No password/MFA handling in the client; reuses the existing, security-hardened web flow.
- ✅ Preserves the offline-first promise — no account required to use the agent.
- ❌ Two states (anon/local vs authenticated) to handle in the UI; mitigated by treating auth as an additive overlay.
- ❌ Device-code grant not yet validated against a real client — a dedicated milestone (D2), not a formality.

---

## ADR-012 — Desktop i18n: FR/EN with auto-detect, account language wins (2026-06-04) — accepted

**Context**: UI is English-only and hard-coded. User wants FR/EN with automatic language detection. aonyx-account stores `UserProfile.language`.

**Decision**: Ship FR/EN message bundles (`fr.json`/`en.json`, EN fallback) with a `t()` helper. Detect OS/browser locale on first run; when signed in, **`UserProfile.language` is authoritative** (and a manual change in the desktop pushes back to the account). Manual override available in Settings.

**Consequences**:
- ✅ Zero-config localized UX; consistent language across a user's devices via the account.
- ❌ All UI strings must be extracted; one-time cost, enforced by a lint pass.

---

## ADR-013 — Register `aonyx-agent` in aonyx-account with FREE/PREMIUM licensing (2026-06-04) — accepted

**Context**: Adding an app to aonyx-account = one entry in `server/config/apps.config.ts`. `imvu-toolkit` (type `electron`, `licensingEnabled:true`, FREE/PREMIUM) is the precedent. The agent code is OSS/MIT and must stay freely usable locally.

**Decision**: Register `aonyx-agent` (`type:'electron'`, `routePrefix:'agent'`, `licensingEnabled:true`, `defaultTier:'FREE'`) mirroring the toolkit. The license **gates cloud/premium features** (sync, multi-device, quotas) — never the local binary. Adds workstream D3 (routes `/api/v1/agent/*` + license service).

**Consequences**:
- ✅ Monetization/cloud-feature path in place from the start, reusing the toolkit's license machinery.
- ✅ Local-first/OSS guarantee intact — no license needed to run the agent locally.
- ❌ FREE/PREMIUM feature split still to be defined (OQ4-bis); more work on the aonyx-account side than a license-free app.

---

## ADR-014 — Desktop frontend: adopt aonyx-rag's React/Vite/Tailwind organization (2026-06-04) — accepted

**Context**: ADR-010 chose vanilla JS + golden-layout. In practice the desktop needs a nav-rail dashboard (Dashboard/Chat/Projets/…/Paramètres), auth, a settings sub-app, KG viz, theming, i18n — a lot of stateful UI. The user's own mature Electron app (aonyx-rag, `H:\Web\RAG`) already solves this with React 18 + Vite + Tailwind + react-router (HashRouter) + react-query + lucide-react, cleanly organized (shell, section views, `ui/` design system, `context/AuthContext`, `hooks/`, `services/`, `config/`). The shared sidebar screenshot is its nav rail. Plan: [`desktop-v2-architecture.md`](desktop-v2-architecture.md). Clean-room (ADR-001/008): mirror the *organization/patterns/conventions*, never aonyx-rag's data or RAG business logic.

**Decision**: Rebuild the desktop renderer as a React + Vite + Tailwind app mirroring aonyx-rag's structure, on Tauri. Electron IPC (`window.electronAPI`/contextBridge) maps to a `services/` layer over `window.__TAURI__.core.invoke`; the existing Rust commands ARE the API. HashRouter for the nav sections (`tauri://` origin, like Electron's `file://`). Supersedes ADR-010 — vanilla + golden-layout abandoned; the nav rail + router is the real answer to "a sidebar like that". Movable panels become an optional later feature (`react-resizable-panels`) inside Chat.

**Consequences**:
- ✅ Proven, maintainable architecture matching the user's reference app; scales to auth/settings/KG/i18n.
- ✅ Rust backend commands unchanged — the hard part is done; only the renderer is rebuilt.
- ❌ Vanilla→React migration is a real lift (phased P0–P5); the in-progress vanilla nav-rail is discarded.
- ❌ Adds a JS toolchain (Vite/npm) to the desktop build (`beforeDevCommand`/`beforeBuildCommand`) — standard for Tauri.

---

## ADR-015 — aonyx-agent FREE/PREMIUM feature split (2026-06-06) — accepted

**Context**: ADR-013 registered `aonyx-agent` with FREE/PREMIUM licensing but left the feature split open (OQ4-bis). The agent is OSS/MIT and local-first (ADR-001/011): the local binary must never be gated; the premium tier monetizes the **cloud layer only**. The split is now live in the aonyx-account registry (`server/config/apps.config.ts` + the prod patch `patches/apps.config.js`, deployed 2026-06-06) and validated by the user.

**Decision**:
- **FREE** (`defaultTier`, `freeMaxDevices: 1`): agent local illimité (offline), mémoire + RAG documentaire locaux, multi-provider avec clés perso, 1 appareil.
- **PREMIUM** (6.99 €/mois · 59.99 €/an, −30 %): sync multi-appareils, sauvegarde cloud chiffrée, embeddings cloud, historique partagé, support prioritaire, accès anticipé.
- The license gates **only** cloud/account features; the local agent (chat, memory, RAG, providers) stays fully usable without any license. **Resolves OQ4-bis** (ADR-013).

**Consequences**:
- ✅ Clear monetization boundary: cloud = payant, local = libre pour toujours — préserve la promesse OSS/MIT + local-first.
- ✅ Encoded once in `apps.config` (source of truth); the desktop reads the tier via `isPremium` (ADR-013) — no per-feature hardcoding.
- ❌ Les fonctions premium (sync, backup cloud, embeddings cloud) ne sont **pas encore construites** : ce split en est la spec ; le gating `isPremium` est un no-op jusqu'à leur arrivée.

---

*Future decisions append here.*
