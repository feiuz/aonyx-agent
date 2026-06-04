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

  const refreshSessions = useCallback(async () => {
    try {
      const list = await agent.listSessions();
      setSessions(Array.isArray(list) ? list : []);
    } catch {
      /* keep previous list */
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
      return true;
    } catch (e) {
      setStatus("err");
      setError(String(e));
      return false;
    }
  }, [refreshSessions]);

  useEffect(() => {
    connect();
  }, [connect]);

  const createSession = useCallback(async () => {
    const rec = await agent.createSession();
    setSessionId(rec.id);
    refreshSessions();
    return rec.id;
  }, [refreshSessions]);

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
