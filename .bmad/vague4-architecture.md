# BMAD — Architecture (Vague 4: Automation API + Windows Desktop)

**Project**: Aonyx Agent
**Phase**: 3 — Architecture
**Date**: 2026-06-02
**Status**: Design locked; ready for V4.1.

---

## 1. Guiding principle — thin shells over the existing core

The API and the desktop are **transports**, not new agents. Both bottom out
in the same library code that powers the CLI/TUI today:

- `aonyx-agent` (lib) — `AgentRunner` (the loop, streaming `TurnEvent`),
  classifier, compaction, approval gate, `Config`, `secrets` (keyring),
  `backup`, `reflect`.
- `aonyx-memory` — `Palace` (KG + diary + hybrid search + time-machine) and
  `SqliteSessionStore`.
- `aonyx-tools` — `ToolRegistry` / `ToolHandler`.
- `aonyx-skills` — `SkillEngine`.

No loop logic is reimplemented. If a behaviour exists in the TUI, the API
calls the same function.

## 2. New components

```
crates/aonyx-api/         NEW — axum REST + WS over the core (library + router)
crates/aonyx-agent/       + ServeChannel::Api, `aonyx serve api` wiring
desktop/                  NEW — Tauri 2 app
  src-tauri/              Rust: Tauri commands, AgentClient {Local, Remote}
  src/                    Web UI (Svelte + Vite), reuses site design tokens
  src-tauri/tauri.conf.json
.github/workflows/
  desktop.yml             NEW — build .msi/.exe on windows-latest, attach to release
```

### 2.1 `aonyx-api` crate

```rust
// Public surface
pub struct ApiState {
    pub runner_factory: Arc<dyn Fn(SessionId) -> AgentRunner + Send + Sync>,
    pub palace: Arc<Palace>,
    pub sessions: Arc<SqliteSessionStore>,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillEngine>,
    pub config: Arc<RwLock<Config>>,
    pub auth: AuthConfig,            // token, allow_destructive, bind addr
}

pub fn build_router(state: ApiState) -> axum::Router; // all /v1 routes + co-mounted OpenAI
```

- **axum 0.7** (already a workspace dep via the `openai-server` feature).
- **Auth**: a `from_fn` middleware checks `Authorization: Bearer`; skips
  `/v1/health`. Token resolution: `Config.api` → keyring `api_token` → env
  `AONYX_API_TOKEN`.
- **Streaming**: `axum::extract::ws` for FR-A8; `axum::response::sse` for
  FR-A9. Both subscribe to `AgentRunner::run_streaming()`'s `TurnEvent`
  stream and serialize each event to a frame. Socket close → `abort` the
  turn (drop the stream / cancel token).
- **Errors**: one `ApiError` enum → `IntoResponse` with proper status +
  `{error:{type,message,detail}}`.
- **OpenAI co-mount**: `Router::merge` the existing
  `aonyx-adapters::openai_server` router so one port serves both. (Move the
  shared handler into a place both can call, or depend on the adapter's
  router builder.)
- **Tests**: `axum::Router` + `tower::ServiceExt::oneshot` for handler-level
  tests with a stub runner/palace (no live LLM).

### 2.2 `aonyx serve api`

New `ServeChannel::Api { port, token, bind }` in the binary. Builds
`ApiState` from the resolved `Config` + `Palace` + `SqliteSessionStore` +
the registered `ToolRegistry` (same construction path as the TUI), then
`axum::serve`. Feature flag **`api`** on `aonyx-agent`, included in the
`-full` binary and the `--features` matrix. Refuses non-loopback bind
without a token (FR-AX4).

### 2.3 WebSocket / SSE protocol

Client → server (WS only): `{"type":"user","content":"…","images":[…]}`,
`{"type":"cancel"}`.

Server → client frames (WS and SSE share the JSON shape):

```json
{"type":"delta","text":"…"}
{"type":"tool_start","name":"fs_read","call_id":"…","args":{…},"class":"safe"}
{"type":"tool_result","call_id":"…","output":"…","error":null}
{"type":"usage","input_tokens":…,"output_tokens":…}
{"type":"done","message_id":"…"}
{"type":"error","message":"…"}
```

These mirror the existing `TurnEvent` variants the TUI already renders, so
the mapping is mechanical.

## 3. Desktop architecture (Tauri 2)

### 3.1 The unifying abstraction — `AgentClient`

The whole point of "hybrid" is one interface, two backends. Defined in
`src-tauri`:

```rust
#[async_trait]
trait AgentClient: Send + Sync {
    async fn list_sessions(&self, project: Option<&str>) -> Result<Vec<SessionMeta>>;
    async fn create_session(&self, req: NewSession) -> Result<SessionId>;
    async fn send_turn(&self, id: &SessionId, msg: UserTurn,
                       tx: Sender<TurnEvent>) -> Result<()>;   // streams
    async fn memory_search(&self, q: &str, k: usize, as_of: Option<Time>) -> Result<Vec<Hit>>;
    async fn kg_entities(&self, …) -> Result<…>;
    async fn diary(&self, project: &str) -> Result<Vec<DiaryEntry>>;
    async fn list_tools(&self) -> Result<Vec<ToolInfo>>;
    async fn list_skills(&self) -> Result<Vec<SkillInfo>>;
    async fn get_config(&self) -> Result<PublicConfig>;
    async fn set_config(&self, patch: ConfigPatch) -> Result<()>;
}

struct LocalClient  { /* embeds AgentRunner + Palace + SessionStore */ }
struct RemoteClient { base: Url, token: String /* reqwest + tokio-tungstenite */ }
```

- **LocalClient** calls the `aonyx-*` crates in-process — identical to how
  the CLI builds them. Streaming turns push `TurnEvent`s to the frontend via
  Tauri's event channel.
- **RemoteClient** calls `aonyx-api` over HTTP (`reqwest`) and WS
  (`tokio-tungstenite`), deserializing the same frames back into
  `TurnEvent`. So `aonyx-api`'s wire types are the contract both sides share
  (put DTOs in `aonyx-api` and depend on it from `src-tauri`).
- The active client is chosen by a **Settings** value (`Local` |
  `Remote{url,token}`); switching re-instantiates the trait object. The UI
  never knows which is active.

### 3.2 Tauri commands → frontend

Thin `#[tauri::command]` wrappers delegate to the current `AgentClient`.
Streaming uses `tauri::ipc::Channel<TurnEvent>` (or `app.emit`) so the
frontend receives deltas live. Commands: `list_sessions`, `create_session`,
`send_turn`, `cancel_turn`, `memory_search`, `kg_entities`, `diary`,
`list_tools`, `list_skills`, `get_config`, `set_config`, `set_connection`,
`setup_provider` (keyring write via `secrets.rs`).

### 3.3 Frontend (web UI)

- **Svelte + Vite** (official Tauri template; lightest SPA, fastest to
  parity). React/Solid acceptable — Svelte chosen for size + simplicity.
- **Design reuse**: copy the site's CSS custom properties (palette, Saira
  display + Inter/mono body, spacing, glows) into the app's stylesheet so
  the desktop matches `agent.aonyx.site` out of the box. Icons: Lucide
  (same as the site).
- **Markdown + code**: a small markdown renderer + a highlighter (e.g.
  `shiki`/`marked`) for chat — matches the TUI's rendered output.
- **KG view**: a lightweight graph (e.g. `d3-force` or `cytoscape`) fed by
  `kg_entities`.

### 3.4 Secrets in the desktop

API keys and the remote API token are stored via the **same keyring** path
(`secrets.rs`) the CLI uses — never written to the Tauri store or exposed to
the WebView. The Settings screen calls a `setup_provider` command that does
the keyring write in Rust.

## 4. Security model

| Surface | Control |
|---|---|
| API auth | Bearer token (keyring/flag/env); `401` without; `/health` open. |
| Network exposure | Bind `127.0.0.1` by default; `0.0.0.0` requires a token or the process refuses to start; log a warning. |
| Dangerous tools over API | `fs_write`/`bash`/`sandbox_exec` etc. invoked via FR-A16 require `api.allow_destructive = true` (default false); the agent loop still applies the approval classifier. |
| Secrets | Never returned/accepted by `/v1/config`; keyring only; never in the WebView. |
| Desktop remote mode | TLS expected for non-loopback URLs; token stored in keyring. |
| CORS | Allow-list (desktop origin `tauri://localhost` + configured origins). |

## 5. Release & packaging

- **API** ships inside the existing `-full` binary (feature `api`) and on
  crates.io with the other crates (publish order: `aonyx-api` after its
  deps `core/memory/llm/tools/skills`, before `aonyx-agent` — which gains an
  optional dep on it for the `api` feature). → **v0.7.0**.
- **Desktop**: new `desktop.yml` on `windows-latest`:
  `tauri build` → `.msi` (WiX) + `.exe` (NSIS) → upload to the GitHub
  Release. Decoupled from the crate release so a desktop rebuild doesn't
  touch crates.io. App icon + metadata in `tauri.conf.json`. Updater keys
  generated; the update feed is optional (documented). Code-signing: ship
  unsigned in V4 with an install note; EV/OV cert later. → **v0.8.0**.
- glibc note is irrelevant for the desktop (Windows); the API binary keeps
  the 2.35 floor from the release pipeline.

## 6. Dependencies (new)

| Crate / pkg | Where | Why |
|---|---|---|
| `axum` (ws, sse) | aonyx-api | already in tree (openai-server) |
| `tower`, `tower-http` (cors, trace) | aonyx-api | middleware |
| `utoipa` (or hand-written) | aonyx-api | OpenAPI spec (FR-AX5) |
| `reqwest`, `tokio-tungstenite` | desktop/src-tauri | RemoteClient |
| `tauri` 2, `tauri-build` | desktop/src-tauri | shell + bundler |
| `svelte`, `vite`, `@tauri-apps/cli` | desktop (npm) | frontend |
| `marked`/`shiki`, `cytoscape`/`d3` | desktop (npm) | chat render + KG view |

## 7. Open questions (decide during V4)

- O1 — Multi-project in one API instance: scope the palace per-request
  (`?project=`) or per-instance? **Lean**: per-request project selector,
  one palace root. (Decide in V4.4.)
- O2 — OpenAPI: `utoipa` derive vs a hand-maintained `openapi.json`?
  Default to `utoipa` if it doesn't fight axum 0.7. (V4.4.)
- O3 — Desktop graph lib (cytoscape vs d3-force): pick in V4.7 by bundle
  size.
- O4 — Updater feed hosting (GitHub Releases `latest.json`): wire keys now,
  enable feed post-V4.

## 8. What stays unchanged

The CLI, TUI, MCP server, chat adapters, and the memory palace formats are
untouched. V4 only **adds** two front doors. A user who never runs
`serve api` or installs the desktop sees no difference.
