import { useEffect, useRef, useState } from "react";
import { MessageSquare, Plus, Send } from "lucide-react";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import * as agent from "../services/agentService";
import Message from "../components/agent/Message";

export default function Chat() {
  const {
    status,
    info,
    error,
    sessions,
    sessionId,
    setSessionId,
    refreshSessions,
    ensureSession,
    createSession,
  } = useAgent();
  const { t } = useI18n();

  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const logRef = useRef(null);
  const taRef = useRef(null);

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!sessionId) {
        setMessages([]);
        return;
      }
      try {
        const rec = await agent.getSession(sessionId);
        if (cancelled) return;
        const msgs = (rec.messages || []).flatMap((m) => {
          if (m.role === "user" && m.content) return [{ role: "user", content: m.content }];
          if (m.role === "assistant" && (m.content || agent.toolNamesOf(m).length))
            return [{ role: "assistant", content: m.content || "", tools: agent.toolNamesOf(m) }];
          return [];
        });
        setMessages(msgs);
      } catch {
        /* ignore */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const grow = () => {
    const ta = taRef.current;
    if (ta) {
      ta.style.height = "auto";
      ta.style.height = Math.min(ta.scrollHeight, 160) + "px";
    }
  };

  const onNew = async () => {
    if (busy) return;
    try {
      await createSession();
      setMessages([]);
    } catch {
      /* ignore */
    }
  };

  const send = async () => {
    const text = input.trim();
    if (!text || busy || status !== "ok") return;
    let sid;
    try {
      sid = await ensureSession();
    } catch {
      return;
    }
    setInput("");
    grow();
    setMessages((m) => [
      ...m,
      { role: "user", content: text },
      { role: "assistant", content: "", tools: [], streaming: true },
    ]);
    setBusy(true);

    let acc = "";
    const tools = [];
    const patchLast = (patch) =>
      setMessages((m) => {
        const copy = [...m];
        copy[copy.length - 1] = { ...copy[copy.length - 1], ...patch };
        return copy;
      });

    try {
      await agent.streamMessage(sid, text, (frame) => {
        switch (frame?.type) {
          case "delta":
            acc += frame.text || "";
            patchLast({ content: acc, streaming: true });
            break;
          case "tool_start":
            if (frame.name) {
              tools.push(frame.name);
              patchLast({ tools: [...tools] });
            }
            break;
          case "done":
            patchLast({ content: acc || frame.reply || "", tools: [...tools], streaming: false });
            break;
          case "error":
            patchLast({ content: frame.message || "stream error", error: true, streaming: false });
            break;
          default:
            break;
        }
      });
      patchLast({ streaming: false });
      refreshSessions();
    } catch (e) {
      patchLast({ content: String(e), error: true, streaming: false });
    } finally {
      setBusy(false);
      taRef.current?.focus();
    }
  };

  const onKey = (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  return (
    <div className="flex h-full">
      {/* conversations sub-panel */}
      <aside className="w-60 flex-shrink-0 flex flex-col border-r border-aonyx-200 dark:border-aonyx-800">
        <div className="flex items-center justify-between h-14 px-3 flex-shrink-0 border-b border-aonyx-200 dark:border-aonyx-800">
          <span className="text-[11px] font-cond uppercase tracking-wider text-aonyx-500">
            {t("chat.conversations")}
          </span>
          <button
            onClick={onNew}
            className="flex items-center gap-1 text-xs px-2 py-1 rounded-md border border-aonyx-300 dark:border-aonyx-700 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50"
          >
            <Plus className="w-3.5 h-3.5" /> {t("chat.new")}
          </button>
        </div>
        <ul className="flex-1 overflow-y-auto p-2 space-y-0.5">
          {sessions.length === 0 && (
            <li className="text-xs text-aonyx-500 px-2 py-1.5">{status === "ok" ? t("chat.none") : "—"}</li>
          )}
          {sessions.map((s) => (
            <li key={s.id}>
              <button
                onClick={() => !busy && setSessionId(s.id)}
                className={`w-full text-left px-2.5 py-2 rounded-md transition-colors ${
                  s.id === sessionId
                    ? "bg-aonyx-200/70 dark:bg-aonyx-800/70 text-aonyx-900 dark:text-aonyx-100"
                    : "text-aonyx-600 dark:text-aonyx-400 hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/50"
                }`}
              >
                <span className="block truncate text-sm">{s.title || t("chat.untitled")}</span>
                <span className="block text-[11px] font-mono text-aonyx-500">
                  {s.turns} {s.turns === 1 ? t("chat.turn") : t("chat.turns")}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      {/* chat column */}
      <section className="flex-1 min-w-0 flex flex-col">
        <header className="flex items-center justify-between h-14 px-5 flex-shrink-0 border-b border-aonyx-200 dark:border-aonyx-800">
          <div className="flex items-center gap-2.5">
            <MessageSquare className="w-5 h-5 text-aonyx-500" strokeWidth={1.75} />
            <h1 className="font-cond uppercase tracking-wide text-lg text-aonyx-900 dark:text-aonyx-100">{t("nav.chat")}</h1>
          </div>
          <div className="flex items-center gap-2 text-xs font-mono text-aonyx-500" title={error || ""}>
            <span
              className={`w-2 h-2 rounded-full ${
                status === "ok" ? "bg-emerald-500" : status === "connecting" ? "bg-amber-500" : "bg-red-500"
              }`}
            />
            {status === "ok" && info
              ? `${info.provider} · ${info.model}`
              : status === "connecting"
                ? t("status.connecting")
                : t("status.offline")}
          </div>
        </header>

        <main ref={logRef} className="flex-1 overflow-y-auto p-6 flex flex-col gap-4">
          {messages.length === 0 ? (
            <div className="m-auto text-center max-w-md text-aonyx-500">
              <h2 className="font-cond uppercase tracking-wider text-2xl text-aonyx-700 dark:text-aonyx-200">
                Aonyx Agent
              </h2>
              <p className="mt-2 text-sm">
                {status === "ok" ? t("chat.empty.ready") : error || t("chat.empty.configure")}
              </p>
            </div>
          ) : (
            messages.map((m, i) => (
              <Message
                key={i}
                role={m.role}
                content={m.content || (m.streaming ? "…" : "")}
                tools={m.tools}
                error={m.error}
                streaming={m.streaming && !m.content}
              />
            ))
          )}
        </main>

        <footer className="flex gap-2 p-4 flex-shrink-0 border-t border-aonyx-200 dark:border-aonyx-800">
          <textarea
            ref={taRef}
            rows={1}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              grow();
            }}
            onKeyDown={onKey}
            disabled={status !== "ok"}
            placeholder={t("chat.placeholder")}
            className="flex-1 resize-none max-h-40 rounded-lg px-3 py-2.5 text-sm select-text bg-white dark:bg-aonyx-950 border border-aonyx-300 dark:border-aonyx-700 focus:outline-none focus:border-primary-500 disabled:opacity-50"
          />
          <button
            onClick={send}
            disabled={busy || status !== "ok" || !input.trim()}
            className="flex items-center justify-center px-4 rounded-lg bg-primary-600 hover:bg-primary-700 text-white disabled:opacity-40 disabled:cursor-not-allowed"
            aria-label={t("chat.new")}
          >
            <Send className="w-4 h-4" />
          </button>
        </footer>
      </section>
    </div>
  );
}
