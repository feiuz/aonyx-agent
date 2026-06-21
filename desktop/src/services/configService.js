import { invoke, safeInvoke } from "../config/bridge";

export const readProviderConfig = () => safeInvoke("read_provider_config", undefined, {});
export const saveProviderConfig = (cfg) => invoke("save_provider_config", { cfg });

// Live model list per provider (Rust list_models). claude-code reuses the local
// Claude Code OAuth session; key-based providers return "API_KEY_REQUIRED".
export const listModels = (provider, base, key) =>
  invoke("list_models", { provider, base, key });

// Relaunch the Claude Code CLI so it refreshes / re-logins its own OAuth session
// (the desktop never touches ~/.claude). Used when claude-code's token expired.
export const claudeLogin = (binary) => invoke("claude_login", { binary });
