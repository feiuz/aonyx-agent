import { useEffect, useRef, useState } from "react";
import { MessageSquare, Plus, Send, Cpu } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import * as agent from "../services/agentService";
import Message from "../components/agent/Message";
import logo from "../assets/logo.png";

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
  const navigate = useNavigate();

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
            <div className="relative m-auto w-full max-w-xl flex flex-col items-center justify-center text-center px-6 py-10">
              <img
                src={logo}
                alt=""
                aria-hidden="true"
                className="pointer-events-none select-none absolute w-72 max-w-[70%] opacity-[0.06] dark:opacity-[0.09]"
                style={{ top: "50%", left: "50%", transform: "translate(-50%, -60%)" }}
              />
              <div className="relative">
                <h2 className="font-cond font-bold uppercase tracking-tight leading-none text-5xl sm:text-6xl text-aonyx-900 dark:text-aonyx-50">
                  Aonyx Agent
                </h2>
                <p className="mt-4 text-sm text-aonyx-500 max-w-sm mx-auto leading-relaxed">
                  {status === "ok" ? t("home.tagline") : error || t("chat.empty.configure")}
                </p>
              </div>
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

        <footer className="p-4 flex-shrink-0">
          <div className="flex items-end gap-2 max-w-3xl mx-auto rounded-2xl border border-aonyx-300 dark:border-aonyx-700 bg-white dark:bg-aonyx-950 px-3 py-2 transition-colors focus-within:border-primary-500 focus-within:ring-1 focus-within:ring-primary-500/25">
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
              className="flex-1 resize-none max-h-40 bg-transparent px-1 py-1.5 text-sm select-text focus:outline-none disabled:opacity-50"
            />
            {info?.model && (
              <button
                onClick={() => navigate("/settings")}
                title={t("nav.settings")}
                className="hidden sm:flex items-center gap-1.5 mb-0.5 shrink-0 max-w-[170px] text-[11px] font-mono text-aonyx-500 border border-aonyx-300 dark:border-aonyx-700 rounded-lg px-2 py-1 hover:bg-aonyx-100 dark:hover:bg-aonyx-900 transition-colors"
              >
                <Cpu className="w-3 h-3 shrink-0" />
                <span className="truncate">{info.model}</span>
              </button>
            )}
            <button
              onClick={send}
              disabled={busy || status !== "ok" || !input.trim()}
              className="flex items-center justify-center w-9 h-9 rounded-xl bg-primary-600 hover:bg-primary-700 text-white disabled:opacity-40 disabled:cursor-not-allowed shrink-0"
              aria-label={t("chat.new")}
            >
              <Send className="w-4 h-4" />
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
