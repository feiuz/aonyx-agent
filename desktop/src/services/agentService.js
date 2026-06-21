import { invoke, channel, isTauri } from "../config/bridge";

// Talks to `aonyx serve api` through the Rust-side Tauri commands (no HTTP/CORS
// in the renderer). Holds the active endpoint chosen by connect().

let endpoint = { url: "http://127.0.0.1:8788", token: "" };

const store = {
  get local() {
    return localStorage.getItem("aonyx.local") !== "0"; // embedded by default
  },
  get url() {
    return localStorage.getItem("aonyx.apiUrl") || "http://127.0.0.1:8788";
  },
  get token() {
    return localStorage.getItem("aonyx.token") || "";
  },
};

const withArgs = (extra) => ({ base: endpoint.url, token: endpoint.token, ...extra });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export { isTauri };

/** Connect: start the embedded agent (or use the remote URL), then probe info. */
export async function connect() {
  if (!isTauri()) throw new Error("Not running inside Tauri");
  endpoint = store.local
    ? { url: await invoke("start_local"), token: "" }
    : { url: store.url, token: store.token };
  const tries = store.local ? 14 : 2; // retry while a fresh local agent binds
  let info;
  for (let i = 0; i < tries; i++) {
    try {
      info = await invoke("api_info", withArgs());
      break;
    } catch (e) {
      if (i === tries - 1) throw e;
      await sleep(350);
    }
  }
  return info;
}

export const listSessions = () => invoke("api_list_sessions", withArgs({ project: null }));
export const getSession = (id) => invoke("api_get_session", withArgs({ session: id }));
export const createSession = () => invoke("api_create_session", withArgs({ project: null }));
export const memorySearch = (q, k = 8) => invoke("api_memory_search", withArgs({ q, k }));
export const kgEntities = (limit = 300) => invoke("api_kg_entities", withArgs({ limit }));
export const kgRelations = (limit = 800) => invoke("api_kg_relations", withArgs({ limit }));
export const tools = () => invoke("api_tools", withArgs());

/** Resolve a paused destructive tool call (interactive approval). */
export const approve = (id, approved) => invoke("api_approve", withArgs({ id, approved }));

/** Enable/disable a tool for the next turn. */
export const toolEnabled = (name, enabled) => invoke("api_tool_enabled", withArgs({ name, enabled }));

/** Registered skills (built-in + user). */
export const skills = () => invoke("api_skills", withArgs());

/** Enable/disable a skill for the next turn. */
export const skillEnabled = (id, enabled) => invoke("api_skill_enabled", withArgs({ id, enabled }));

/** Stream a turn. onFrame gets {type:"delta"|"tool_start"|"done"|"error", …}.
 *  `attachments` is an optional array of {type:"image", media_type, data}. */
export async function streamMessage(session, content, attachments, onFrame) {
  const ch = channel();
  if (ch) ch.onmessage = onFrame;
  return invoke(
    "api_stream",
    withArgs({ session, content, attachments: attachments?.length ? attachments : null, onEvent: ch }),
  );
}

export const toolNamesOf = (msg) =>
  (msg?.tool_calls || []).map((tc) => tc?.name).filter(Boolean);

// Reconstruct tool events from a persisted assistant message (history). Args are
// stored on each ToolCall, so the delegation block survives a reload; the result
// summary isn't (it lives in the separate tool-result message) — done/ok only.
export const toolEventsOf = (msg) =>
  (msg?.tool_calls || [])
    .filter((tc) => tc?.name)
    .map((tc) => ({ name: tc.name, args: tc.args, done: true, ok: true }));
