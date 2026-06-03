# BMAD — PRD (Vague 4: Automation API + Windows Desktop)

**Project**: Aonyx Agent
**Phase**: 2 — Product
**Date**: 2026-06-02
**Status**: Planned (post v0.6.0). Targets **v0.7.0** (API) and **v0.8.0** (Desktop).

---

## Personas

| Persona | Need | What V4 gives them |
|---|---|---|
| **P1 — Integrator** (CI, scripts, backend dev) | Drive the agent from code: spin sessions, stream turns, query memory, invoke tools — without a terminal. | `aonyx-api`: REST + WS, one bearer token, OpenAPI spec, language-agnostic. |
| **P2 — Ops / platform** | Run a shared Aonyx on a server, let teammates connect. | `aonyx serve api` on a host + the desktop's **Remote** mode. |
| **P3 — Desktop user** (non-terminal) | A real app: chat, see the memory palace, change provider — no CLI. | Tauri Windows app, local agent embedded, `.msi` installer. |
| **P4 — Power user** (already CLI) | One GUI that can talk to either a local agent or a remote server. | Hybrid desktop: local ↔ remote toggle. |

## Functional requirements

### A. Automation API (`aonyx-api`, served by `aonyx serve api`)

All routes under `/v1`, JSON, bearer auth (`Authorization: Bearer <token>`),
CORS allow-list (desktop origins + configurable). The existing
OpenAI-compatible routes are **co-mounted** so the same port serves both.

| # | Method + path | Purpose |
|---|---|---|
| FR-A1 | `GET /v1/health` | Liveness (no auth). |
| FR-A2 | `GET /v1/info` | Version, active provider/model, enabled features, capability flags (auth required). |
| FR-A3 | `GET /v1/sessions` | List sessions (filter `?project=`). |
| FR-A4 | `POST /v1/sessions` | Create `{project?, system_prompt?, model?}` → `{id}`. |
| FR-A5 | `GET /v1/sessions/{id}` | Session detail + message history. |
| FR-A6 | `DELETE /v1/sessions/{id}` | Delete a session. |
| FR-A7 | `POST /v1/sessions/{id}/messages` | Send a user turn. Blocking → final assistant message + tool trace. |
| FR-A8 | `GET /v1/sessions/{id}/stream` (WebSocket) | Bidirectional: client sends user turns, server streams `TurnEvent` frames (delta, tool_start, tool_result, usage, done). |
| FR-A9 | `POST /v1/sessions/{id}/messages?stream=true` (SSE) | One-shot streamed turn for clients that prefer SSE over WS. |
| FR-A10 | `GET /v1/memory/search?q=&k=&as_of=` | Hybrid search (BM25 + vectors + RRF), optional time-machine. |
| FR-A11 | `GET /v1/memory/kg/entities` · `GET /v1/memory/kg/entities/{name}` | KG browse + relations. |
| FR-A12 | `GET /v1/memory/diary?project=` · `POST /v1/memory/diary` | Read / append diary. |
| FR-A13 | `POST /v1/memory/ingest` | Ingest text/file into the palace. |
| FR-A14 | `POST /v1/memory/backup` · `POST /v1/memory/restore` | Encrypted palace backup/restore (reuse `backup.rs`). |
| FR-A15 | `GET /v1/tools` | List tools (name, description, JSON schema, safety class, enabled). |
| FR-A16 | `POST /v1/tools/{name}/invoke` | Invoke a tool directly (policy-gated; Destructive tools require an explicit allow flag in config). |
| FR-A17 | `GET /v1/skills` · `POST /v1/skills/{id}/toggle` | List + enable/disable skills. |
| FR-A18 | `GET /v1/config` · `PATCH /v1/config` | Read / update non-secret config (provider, model, policy). **Never** returns or accepts keyring secrets. |
| FR-A19 | `POST /v1/chat/completions` (+ `/v1/responses`) | Existing OpenAI-compat surface, co-mounted unchanged. |

**Cross-cutting:**
- FR-AX1 Auth: bearer token from keyring (`api_token`) / `--token` / `AONYX_API_TOKEN`; `401` otherwise. `/health` is open.
- FR-AX2 Errors: RFC-7807-ish `{error:{type,message,detail}}`, correct HTTP codes.
- FR-AX3 Concurrency: many sessions in flight; per-session serialization of turns.
- FR-AX4 Binding: defaults to `127.0.0.1`; binding `0.0.0.0` prints a security warning and refuses to start without a token.
- FR-AX5 OpenAPI: serve `/v1/openapi.json` + a short hand-written reference in the docs.

### B. Windows Desktop (Tauri 2, `desktop/`)

| # | Screen / feature | Detail |
|---|---|---|
| FR-B1 | **Chat** | Streaming reply (markdown + code highlight), tool-activity inline (start/result), stop button, copy, attach image (vision). |
| FR-B2 | **Sessions sidebar** | List/search/create/rename/delete/fork; per-project grouping; resume. |
| FR-B3 | **Memory palace viewer** | KG graph (entities/relations, clickable), diary timeline, hybrid-search box with `as_of` slider. |
| FR-B4 | **Settings** | Provider + model picker; API key entry → **OS keyring** (never plaintext); tool approval policy; theme. |
| FR-B5 | **Connection switch** | **Local** (embedded agent) or **Remote** (`aonyx serve api` URL + token); status indicator; the rest of the UI is identical in both modes. |
| FR-B6 | **Tools / skills panels** | View tools + classes; toggle skills; per-session tool enable. |
| FR-B7 | **First-run** | If no provider configured, a wizard mirroring `aonyx setup` (provider → key → live test). |
| FR-B8 | **Packaging** | `.msi` (WiX) + `.exe` (NSIS) via Tauri bundler; app icon; Start-menu entry; updater wired (feed optional). |

## Non-functional requirements

- NFR1 API turn overhead < 50 ms vs calling the loop directly (excluding LLM time).
- NFR2 WS streams first delta as soon as the loop yields it (no buffering).
- NFR3 Desktop cold start < 3 s; idle RAM reasonable for an embedded agent.
- NFR4 Desktop installer `.msi` < 30 MB (the embedded agent dominates).
- NFR5 Security: no secret ever crosses the API or lands in the WebView; localhost-default; token required for non-loopback.
- NFR6 Cross-platform-ready: the Tauri app builds on macOS/Linux too (only Windows packaging is in V4 scope).
- NFR7 `clippy --all-features -D warnings` clean; tests for the API layer; CI green on the existing matrix + a `windows-latest` desktop job.

## Success metrics

| ID | Metric | Target |
|---|---|---|
| V4-M1 | `curl` round-trip: create session → stream a turn | works with one bearer token |
| V4-M2 | OpenAI SDK (`openai` python) against `aonyx serve api` | drop-in success (regression-free) |
| V4-M3 | API turn overhead vs direct loop | < 50 ms p50 |
| V4-M4 | Desktop cold start → first paint | < 3 s |
| V4-M5 | Desktop `.msi` size | < 30 MB |
| V4-M6 | Local ↔ Remote toggle | both modes drive a full chat turn |
| V4-M7 | OpenAPI spec validates + reference docs published (EN/FR) | yes |

## Risk register

| ID | Risk | Mitigation |
|---|---|---|
| V4-R1 | Embedding the full core (fastembed/ONNX, sqlite) in a Tauri build bloats/breaks the Windows build. | Reuse the proven `aonyx-agent` deps; `ort` already builds on `windows-latest` in CI; measure size early (V4.5). Make embeddings the same opt as the lean/full split if needed. |
| V4-R2 | Exposing `bash`/`sandbox_exec`/`fs_write` over HTTP is a real RCE surface. | Localhost default; token required off-loopback; Destructive tools gated behind an explicit `api.allow_destructive` flag (default false); reuse the approval classifier. |
| V4-R3 | WS backpressure / dropped clients leak agent runs. | Bounded channels; cancel the turn on socket close; timeouts. |
| V4-R4 | Tauri 2 + a new frontend framework is unfamiliar (team did Astro, not SPA). | Use Svelte + Vite (smallest learning curve, official Tauri template) and copy the site's CSS tokens for instant visual parity. |
| V4-R5 | Windows code-signing: unsigned `.msi` triggers SmartScreen. | Ship unsigned first with a documented install note; add an EV/OV cert later (out of V4 scope, flagged). |
| V4-R6 | Two new release artifacts (API in `-full`, desktop `.msi`) complicate the pipeline. | API rides the existing `-full` binary; desktop gets its own `desktop.yml` job, decoupled from the crate release. |

## Phase breakdown (BMAD stories → execution)

Maps to the existing phase-letter convention. **v0.7.0 = API**, **v0.8.0 = Desktop**.

| Phase | Title | Output |
|---|---|---|
| **V4.1** | `aonyx-api` scaffold | New crate; axum router + `ApiState`; `/health`, `/info`; bearer-auth middleware; error type; unit tests. |
| **V4.2** | Sessions + turns (blocking) | FR-A3…A7 over `SessionStore` + `AgentRunner`; integration test. |
| **V4.3** | Streaming | WS (FR-A8) + SSE (FR-A9): map `TurnEvent` → frames; cancel-on-close. |
| **V4.4** | Memory + tools + skills + config | FR-A10…A18 over `Palace`/`ToolRegistry`/`SkillEngine`; OpenAPI (FR-AX5). |
| **V4.5** | Wire `aonyx serve api` + co-mount OpenAI | `ServeChannel::Api`; co-mount existing routes; `--token`, bind warning; docs; **cut v0.7.0**. |
| **V4.6** | Desktop scaffold | Tauri 2 + Svelte/Vite; design tokens from the site; `AgentClient` trait + `LocalClient` (embedded); **Chat** screen streaming (FR-B1). |
| **V4.7** | Desktop core screens | Sessions (FR-B2), Memory viewer (FR-B3), Settings + keyring (FR-B4), first-run wizard (FR-B7). |
| **V4.8** | Desktop hybrid + packaging | `RemoteClient` → `aonyx-api`; connection switch (FR-B5); Tauri bundler `.msi`/`.exe`; `desktop.yml` release job; docs; **cut v0.8.0**. |

Each phase ends with the standard ritual: `cargo build` + `clippy --all-features -D warnings` + tests + CI green + CHANGELOG + RAG diary entry.

## Acceptance criteria

**API (v0.7.0):**
- [ ] `aonyx serve api --port 8788` boots; `GET /v1/health` 200 without auth.
- [ ] With a bearer token: create a session, POST a message, get a reply.
- [ ] A WebSocket client receives streamed deltas + tool events for a turn.
- [ ] `GET /v1/memory/search?q=…` returns hybrid results; `GET /v1/tools` lists tools with schemas.
- [ ] The OpenAI Python SDK pointed at the server completes a chat (co-mount intact).
- [ ] `/v1/config` neither returns nor accepts keyring secrets; binding `0.0.0.0` without a token refuses to start.
- [ ] `/v1/openapi.json` validates; reference docs live (EN/FR).

**Desktop (v0.8.0):**
- [ ] `.msi` installs on Windows 10/11; app launches < 3 s.
- [ ] First-run wizard configures a provider; key lands in the OS keyring.
- [ ] A chat turn streams in the window with inline tool activity.
- [ ] The memory viewer shows the KG + diary; search works.
- [ ] Toggling Local ↔ Remote drives a turn in both modes.
- [ ] CI builds the `.msi` on `windows-latest` and attaches it to the release.
