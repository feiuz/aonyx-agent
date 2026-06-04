// Thin wrapper over Tauri's `invoke` — the renderer's only door to the Rust
// backend. Analogous to aonyx-rag's config/apiClient.js, but the transport is
// Tauri commands (no HTTP, no CORS) instead of axios. Services build on top.

const core = typeof window !== "undefined" ? window.__TAURI__?.core : undefined;

/** True when running inside the Tauri webview (false in a plain browser/preview). */
export const isTauri = () => !!core?.invoke;

/** Invoke a Rust command; throws if not in Tauri or the command errors. */
export async function invoke(cmd, args) {
  if (!core?.invoke) throw new Error("Not running inside Tauri");
  return core.invoke(cmd, args);
}

/** Invoke and swallow errors, returning `fallback` instead. */
export async function safeInvoke(cmd, args, fallback = null) {
  try {
    return await invoke(cmd, args);
  } catch {
    return fallback;
  }
}

/** A Tauri Channel for streaming (chat token stream, P1). Null outside Tauri. */
export function channel() {
  return core?.Channel ? new core.Channel() : null;
}
