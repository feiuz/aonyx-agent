# BMAD — Brief (Vague 4: Automation API + Windows Desktop)

**Project**: Aonyx Agent
**Phase**: 1 — Brief
**Date**: 2026-06-02
**Status**: Approved (decisions locked below)

---

## Problem

Aonyx Agent is a complete CLI/TUI product (Vagues 1–3 shipped: memory
palace, multi-provider, tools, MCP, chat adapters, browser, multimodal,
cloud-sync, self-evolution). Two audiences are still unserved:

1. **Integrators / automation** — there is no first-class programmatic
   surface. The only HTTP today is the OpenAI-compatible *chat* shim
   (`aonyx serve openai`) and the MCP server (JSON-RPC, tool-shaped).
   Neither lets a script drive sessions, the memory palace, tools, skills,
   or config over a clean REST/streaming API.
2. **Non-terminal users** — the product is terminal-only. A graphical
   Windows desktop app would make the memory-palace agent approachable.

## Goals

| ID | Goal |
|---|---|
| G1 | A full **REST + WebSocket automation API** over the agent (sessions, streaming turns, memory palace, tools, skills, config), bearer-authed, launched by `aonyx serve api`. |
| G2 | A **native Windows Desktop app** (Tauri 2) — chat + memory-palace UI reusing the site's design language, shipped as a signed-ready `.msi`. |
| G3 | **Hybrid** desktop: works fully offline by embedding the agent in-process, *and* can connect to a remote Aonyx API (drive an agent on a server). |
| G4 | Reuse the existing core — `aonyx-agent` (AgentRunner), `aonyx-memory` (Palace + SessionStore), `aonyx-tools`, `aonyx-skills`, keyring, config — with **no forks** of the loop. |
| G5 | Ship via CI: `aonyx serve api` in the `-full` binaries + crates.io; the desktop `.msi` as a GitHub Release artifact. |

## Locked decisions (the BMAD forks)

- **Desktop stack** → **Tauri 2** (Rust backend + web UI). Reuse the site's
  design (Saira, mono palette, Lucide); Rust core called directly; small
  native installer.
- **API shape** → **full REST + WebSocket** in a new `aonyx-api` crate
  (axum), not a chat-only extension or MCP reuse.
- **Desktop ↔ core** → **hybrid**: embed `aonyx-agent` in-process for local
  mode, *and* a remote client for `aonyx serve api`. The UI talks to one
  `AgentClient` interface with `Local` and `Remote` implementations.

## Scope (Vague 4)

**In:**
- `aonyx-api` crate: REST + WS, bearer auth, CORS, the endpoint set in the
  PRD; co-mounts the existing OpenAI-compat routes.
- `aonyx serve api --port [--token]` command (feature `api`, in `-full`).
- Tauri 2 desktop app (`desktop/`): Chat (streaming), Sessions, Memory
  palace viewer (KG + diary + search), Settings (provider/model/keyring,
  local-vs-remote connection).
- Windows packaging: `.msi` + NSIS `.exe` via the Tauri bundler; a
  `desktop.yml` release workflow on `windows-latest`.
- Docs (site + README) + an OpenAPI description of the API.

**Out (later):**
- macOS / Linux desktop bundles (the Tauri app is cross-platform, but V4
  targets Windows packaging first).
- Multi-tenant / hosted API service, RBAC, rate limiting beyond a token.
- Desktop auto-update server (the updater is wired but the release feed is
  optional).
- Mobile.

## Non-goals

- No rewrite of the agent loop or memory palace — the API and desktop are
  *thin shells* over the existing crates.
- The API does **not** expose keyring secrets (read or write).
- The desktop does not bundle a browser engine beyond the OS WebView2
  (Tauri uses the system WebView).

## Success criteria (high level — detail in PRD)

- A script can: create a session, stream a turn, search the palace, list +
  invoke a tool, all over HTTP, with one bearer token.
- The OpenAI SDK still works against the same server (drop-in).
- The desktop app cold-starts < 3 s, streams a chat reply, browses the KG,
  and switches between a local agent and a remote API — shipped as a `.msi`.
