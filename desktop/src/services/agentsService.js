import { invoke, safeInvoke } from "../config/bridge";

// Custom sub-agents (ADR-017 / MA2): CRUD over ~/.aonyx/agents/*.AGENT.md via
// the Rust side. Built-in presets live in the agent binary (always available).
export const agentsList = () => safeInvoke("agents_list", undefined, { dir: "", agents: [] });
export const agentsSave = (agent) => invoke("agents_save", { agent });
export const agentsDelete = (id) => invoke("agents_delete", { id });
