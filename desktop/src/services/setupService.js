import { invoke, safeInvoke, channel } from "../config/bridge";
import { listModels } from "./configService";

// First-run wizard (ADR-016). setup_state gates the app; save_setup persists the
// full choice set (provider + [rag] backend/embeddings + setup_complete) into
// ~/.aonyx/config.toml. start_local/api_info drive the bootstrap finale.

export const setupState = () => safeInvoke("setup_state", undefined, { configured: true });
export const saveSetup = (cfg) => invoke("save_setup", { cfg });
export const startLocal = () => invoke("start_local");
export const apiInfo = (base, token = "") => invoke("api_info", { base, token });

// Download the local embedding model, streaming fastembed's progress (W4).
// onEvent receives { phase:"downloading", downloaded, total, pct } | { phase:"done" }.
export function prepareEmbeddings(onEvent) {
  const ch = channel();
  if (ch) ch.onmessage = onEvent;
  return invoke("prepare_embeddings", { onEvent: ch });
}

export { listModels };
