import { useEffect, useRef, useState } from "react";
import { Send, Cpu, Plus, Mic, X, Loader2 } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import * as agent from "../services/agentService";
import Message from "../components/agent/Message";
import ApprovalCard from "../components/agent/ApprovalCard";
import logo from "../assets/logo.png";

// Rough token estimate (~4 chars/token) — good enough for the live activity line
// and the context gauge until the backend surfaces real usage.
const estTokens = (s) => Math.ceil((s || "").length / 4);
const fmtTokens = (n) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));
const fmtElapsed = (s) => (s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`);
const contextWindow = (model) => {
  if (!model) return 200000;
  if (/gpt-4o|gpt-4-turbo|gpt-4|gpt-5/i.test(model)) return 128000;
  if (/gpt-3\.5/i.test(model)) return 16000;
  return 200000; // claude opus/sonnet + default
};

export default function Chat() {
  const { status, info, error, sessionId, refreshSessions, ensureSession, setUsage } = useAgent();
  const { t, lang } = useI18n();
  const navigate = useNavigate();

  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [approvals, setApprovals] = useState([]);
  const [attachments, setAttachments] = useState([]);
  const [listening, setListening] = useState(false);
  const [turnElapsed, setTurnElapsed] = useState(0);
  const logRef = useRef(null);
  const taRef = useRef(null);
  const fileRef = useRef(null);
  const recognitionRef = useRef(null);

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);

  // Live turn timer for the activity line (ticks only while a turn runs).
  useEffect(() => {
    if (!busy) {
      setTurnElapsed(0);
      return;
    }
    const start = Date.now();
    const id = setInterval(() => setTurnElapsed(Math.floor((Date.now() - start) / 1000)), 1000);
    return () => clearInterval(id);
  }, [busy]);

  // Estimated context usage → bottom status bar.
  useEffect(() => {
    const tokens = messages.reduce((n, m) => n + estTokens(m.content), 0);
    setUsage({ tokens, max: contextWindow(info?.model) });
  }, [messages, info, setUsage]);

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
          if (m.role === "assistant" && (m.content || agent.toolEventsOf(m).length))
            return [{ role: "assistant", content: m.content || "", events: agent.toolEventsOf(m) }];
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

  const send = async () => {
    const text = input.trim();
    if ((!text && attachments.length === 0) || busy || status !== "ok") return;
    let sid;
    try {
      sid = await ensureSession();
    } catch {
      return;
    }
    const atts = attachments.map((a) => ({ type: "image", media_type: a.media_type, data: a.data }));
    setInput("");
    setAttachments([]);
    grow();
    setMessages((m) => [
      ...m,
      { role: "user", content: text || `📎 ${atts.length}` },
      { role: "assistant", content: "", events: [], streaming: true },
    ]);
    setBusy(true);
    setApprovals([]);

    let acc = "";
    const events = [];
    const patchLast = (patch) =>
      setMessages((m) => {
        const copy = [...m];
        copy[copy.length - 1] = { ...copy[copy.length - 1], ...patch };
        return copy;
      });

    try {
      await agent.streamMessage(sid, text, atts, (frame) => {
        switch (frame?.type) {
          case "delta":
            acc += frame.text || "";
            patchLast({ content: acc, streaming: true });
            break;
          case "tool_start":
            if (frame.name) {
              events.push({ name: frame.name, args: frame.args, done: false });
              patchLast({ events: [...events] });
            }
            break;
          case "tool_end":
            for (let i = events.length - 1; i >= 0; i--) {
              if (events[i].name === frame.name && !events[i].done) {
                events[i] = { ...events[i], ok: frame.ok, summary: frame.summary, done: true };
                break;
              }
            }
            patchLast({ events: [...events] });
            break;
          case "approval_request":
            setApprovals((a) => [
              ...a.filter((x) => x.id !== frame.id),
              { id: frame.id, name: frame.name, args: frame.args, class: frame.class },
            ]);
            break;
          case "done":
            patchLast({ content: acc || frame.reply || "", events: [...events], streaming: false });
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
      setApprovals([]);
      taRef.current?.focus();
    }
  };

  const decide = async (id, approved) => {
    setApprovals((a) => a.filter((x) => x.id !== id));
    try {
      await agent.approve(id, approved);
    } catch {
      /* the server-side timeout denies if this never lands */
    }
  };

  const pickFiles = () => fileRef.current?.click();
  const onFiles = (e) => {
    Array.from(e.target.files || []).forEach((file) => {
      if (!file.type.startsWith("image/")) return;
      const reader = new FileReader();
      reader.onload = () => {
        const data = String(reader.result).split(",")[1] || "";
        setAttachments((a) => [...a, { name: file.name, media_type: file.type, data, url: reader.result }]);
      };
      reader.readAsDataURL(file);
    });
    e.target.value = "";
  };
  const removeAttachment = (i) => setAttachments((a) => a.filter((_, j) => j !== i));

  const voiceSupported =
    typeof window !== "undefined" && !!(window.SpeechRecognition || window.webkitSpeechRecognition);
  const toggleVoice = () => {
    if (!voiceSupported) return;
    if (listening) {
      recognitionRef.current?.stop();
      return;
    }
    const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
    const rec = new SR();
    rec.lang = lang === "fr" ? "fr-FR" : "en-US";
    rec.interimResults = true;
    rec.continuous = false;
    const base = input ? input + " " : "";
    rec.onresult = (e) => {
      let txt = "";
      for (let i = 0; i < e.results.length; i++) txt += e.results[i][0].transcript;
      setInput(base + txt);
      grow();
    };
    rec.onend = () => setListening(false);
    rec.onerror = () => setListening(false);
    recognitionRef.current = rec;
    setListening(true);
    rec.start();
  };

  const onKey = (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  return (
    <section className="flex flex-col h-full">
        <main ref={logRef} className="flex-1 overflow-y-auto p-6 flex flex-col gap-4">
          {messages.length > 0 && status !== "ok" && (
            <div className="flex items-center justify-center gap-2 text-xs font-mono text-aonyx-500">
              <span className={`w-2 h-2 rounded-full ${status === "connecting" ? "bg-amber-500" : "bg-red-500"}`} />
              {status === "connecting" ? t("status.connecting") : error || t("status.offline")}
            </div>
          )}
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
                events={m.events}
                error={m.error}
                streaming={m.streaming && !m.content}
              />
            ))
          )}
          {approvals.map((req) => (
            <ApprovalCard key={req.id} req={req} onDecide={decide} />
          ))}
          {busy &&
            (() => {
              const last = messages[messages.length - 1];
              const tokens = estTokens(last?.content);
              const tasks = (last?.events || []).filter((e) => !e.done).length + approvals.length;
              const running = (last?.events || []).find((e) => !e.done);
              const activity = approvals.length
                ? t("turn.approval")
                : running
                  ? running.name
                  : last?.content
                    ? t("turn.writing")
                    : t("turn.thinking");
              return (
                <div className="flex items-center gap-2 text-xs text-aonyx-500 px-1 self-start">
                  <Loader2 className="w-3.5 h-3.5 animate-spin text-primary-500 flex-shrink-0" />
                  <span className="font-mono">{fmtElapsed(turnElapsed)}</span>
                  <span>· {fmtTokens(tokens)} tokens</span>
                  {tasks > 0 && (
                    <span>
                      · {tasks} {t("turn.tasks")}
                    </span>
                  )}
                  <span className="text-aonyx-400 truncate">· {activity}…</span>
                </div>
              );
            })()}
        </main>

        <footer className="p-4 flex-shrink-0">
          <div className="max-w-3xl mx-auto">
            {attachments.length > 0 && (
              <div className="flex flex-wrap gap-2 mb-2 px-1">
                {attachments.map((a, i) => (
                  <div key={i} className="relative">
                    <img
                      src={a.url}
                      alt={a.name}
                      className="w-14 h-14 object-cover rounded-lg border border-aonyx-300 dark:border-aonyx-700"
                    />
                    <button
                      onClick={() => removeAttachment(i)}
                      className="absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full bg-aonyx-800 text-white flex items-center justify-center hover:bg-aonyx-900"
                    >
                      <X className="w-2.5 h-2.5" />
                    </button>
                  </div>
                ))}
              </div>
            )}
            <div className="flex items-end gap-1 rounded-2xl border border-aonyx-300 dark:border-aonyx-700 bg-white dark:bg-aonyx-950 pl-1.5 pr-2 py-1.5 transition-colors focus-within:border-primary-500 focus-within:ring-1 focus-within:ring-primary-500/25">
              <input ref={fileRef} type="file" accept="image/*" multiple className="hidden" onChange={onFiles} />
              <button
                onClick={pickFiles}
                disabled={status !== "ok"}
                title={t("chat.attach")}
                className="flex items-center justify-center w-8 h-8 rounded-lg text-aonyx-500 hover:bg-aonyx-100 dark:hover:bg-aonyx-900 disabled:opacity-40 shrink-0 mb-0.5"
              >
                <Plus className="w-5 h-5" strokeWidth={1.75} />
              </button>
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
                  className="hidden sm:flex items-center gap-1.5 mb-0.5 shrink-0 max-w-[150px] text-[11px] font-mono text-aonyx-500 border border-aonyx-300 dark:border-aonyx-700 rounded-lg px-2 py-1 hover:bg-aonyx-100 dark:hover:bg-aonyx-900 transition-colors"
                >
                  <Cpu className="w-3 h-3 shrink-0" />
                  <span className="truncate">{info.model}</span>
                </button>
              )}
              {voiceSupported && (
                <button
                  onClick={toggleVoice}
                  disabled={status !== "ok"}
                  title={t("chat.voice")}
                  className={`flex items-center justify-center w-8 h-8 rounded-lg shrink-0 mb-0.5 disabled:opacity-40 ${
                    listening
                      ? "bg-red-500/15 text-red-500 animate-pulse"
                      : "text-aonyx-500 hover:bg-aonyx-100 dark:hover:bg-aonyx-900"
                  }`}
                >
                  <Mic className="w-[18px] h-[18px]" strokeWidth={1.75} />
                </button>
              )}
              <button
                onClick={send}
                disabled={busy || status !== "ok" || (!input.trim() && attachments.length === 0)}
                className="flex items-center justify-center w-9 h-9 rounded-xl bg-primary-600 hover:bg-primary-700 text-white disabled:opacity-40 disabled:cursor-not-allowed shrink-0"
                aria-label={t("chat.new")}
              >
                <Send className="w-4 h-4" />
              </button>
            </div>
          </div>
        </footer>
      </section>
  );
}
