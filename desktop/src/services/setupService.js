import { invoke, safeInvoke } from "../config/bridge";
import { listModels } from "./configService";

// First-run wizard (ADR-016). setup_state gates the app; save_setup persists the
// full choice set (provider + [rag] backend/embeddings + setup_complete) into
// ~/.aonyx/config.toml. start_local/api_info drive the bootstrap finale.

export const setupState = () => safeInvoke("setup_state", undefined, { configured: true });
export const saveSetup = (cfg) => invoke("save_setup", { cfg });
export const startLocal = () => invoke("start_local");
export const apiInfo = (base, token = "") => invoke("api_info", { base, token });

export { listModels };
