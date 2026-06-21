import { createContext, useCallback, useContext, useEffect, useState } from "react";
import * as agent from "../services/agentService";
import { isTauri } from "../config/bridge";

const AgentContext = createContext(null);

export function AgentProvider({ children }) {
  const [status, setStatus] = useState("idle"); // idle | connecting | ok | err
  const [info, setInfo] = useState(null);
  const [error, setError] = useState(null);
  const [sessions, setSessions] = useState([]);
  const [sessionId, setSessionId] = useState(null);
  // Estimated context usage (tokens of the active conversation vs the model's
  // window) — surfaced in the bottom status bar. Chat updates it.
  const [usage, setUsage] = useState({ tokens: 0, max: 200000 });
  // Active RAG project for new conversations (memory is scoped per project).
  const [project, setProjectState] = useState(() => localStorage.getItem("aonyx.project") || "");
  const [projects, setProjects] = useState([]);
  const setProject = useCallback((p) => {
    setProjectState(p || "");
    localStorage.setItem("aonyx.project", p || "");
  }, []);

  const refreshSessions = useCallback(async () => {
    try {
      const list = await agent.listSessions();
      setSessions(Array.isArray(list) ? list : []);
    } catch {
      /* keep previous list */
    }
  }, []);

  const refreshProjects = useCallback(async () => {
    try {
      const r = await agent.projects();
      setProjects(Array.isArray(r) ? r : []);
    } catch {
      /* ignore */
    }
  }, []);

  const connect = useCallback(async () => {
    if (!isTauri()) {
      setStatus("err");
      setError("Aperçu navigateur — lance l'app via Tauri.");
      return false;
    }
    setStatus("connecting");
    setError(null);
    try {
      const i = await agent.connect();
      setInfo(i);
      setStatus("ok");
      await refreshSessions();
      refreshProjects();
      return true;
    } catch (e) {
      setStatus("err");
      setError(String(e));
      return false;
    }
  }, [refreshSessions, refreshProjects]);

  useEffect(() => {
    connect();
  }, [connect]);

  const createSession = useCallback(async () => {
    const rec = await agent.createSession(project || null);
    setSessionId(rec.id);
    refreshSessions();
    return rec.id;
  }, [refreshSessions, project]);

  const ensureSession = useCallback(async () => {
    if (sessionId) return sessionId;
    return createSession();
  }, [sessionId, createSession]);

  return (
    <AgentContext.Provider
      value={{
        status,
        info,
        error,
        sessions,
        sessionId,
        setSessionId,
        usage,
        setUsage,
        project,
        setProject,
        projects,
        refreshProjects,
        connect,
        refreshSessions,
        createSession,
        ensureSession,
      }}
    >
      {children}
    </AgentContext.Provider>
  );
}

export const useAgent = () => useContext(AgentContext);
