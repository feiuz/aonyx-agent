import { invoke, safeInvoke } from "../config/bridge";

export const readProviderConfig = () => safeInvoke("read_provider_config", undefined, {});
export const saveProviderConfig = (cfg) => invoke("save_provider_config", { cfg });

// Live model list per provider (Rust list_models). claude-code reuses the local
// Claude Code OAuth session; key-based providers return "API_KEY_REQUIRED".
export const listModels = (provider, base, key) =>
  invoke("list_models", { provider, base, key });
