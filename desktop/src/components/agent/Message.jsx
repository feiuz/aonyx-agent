import { useState } from "react";
import { CornerDownRight, ChevronDown, Loader2, Check, X } from "lucide-react";
import Markdown from "./Markdown";
import { useI18n } from "../../context/LanguageContext";

// The streamed ToolEnd summary may arrive as the legacy `{"agent":…,"reply":"…"}`
// wrapper (and be truncated mid-string by the 120-char cap). Pull the reply out
// so the block shows prose, not JSON. A plain-string summary passes through.
function resultText(s) {
  if (!s) return s;
  const t = s.trimStart();
  const i = t.indexOf('"reply":');
  if (t.startsWith("{") && i !== -1) {
    let r = t.slice(i + '"reply":'.length).trim();
    if (r.startsWith('"')) r = r.slice(1);
    r = r.replace(/"\s*}?\s*…?$/, "");
    r = r.replace(/\\"/g, '"').replace(/\\n/g, " ").replace(/\\t/g, " ").replace(/\\\\/g, "\\");
    return r.trim() + (s.endsWith("…") ? "…" : "");
  }
  return s;
}

// One architect→sub-agent delegation (the `dispatch_agent` built-in). Collapsed
// by default: agent name + status; expand for the task and the returned result.
function Delegation({ ev }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const name = ev.args?.agent || "agent";
  const task = ev.args?.task || "";
  return (
    <div className="w-full rounded-lg border border-primary-500/30 bg-primary-50/50 dark:bg-primary-950/20 overflow-hidden">
      <button
        onClick={() => setOpen((o) => !o)}
        className="w-full flex items-center gap-2 px-2.5 py-1.5 text-xs hover:bg-primary-500/5"
      >
        <CornerDownRight className="w-3.5 h-3.5 text-primary-500 flex-shrink-0" />
        <span className="font-medium text-primary-700 dark:text-primary-300 flex-shrink-0">{name}</span>
        {ev.done === false ? (
          <Loader2 className="w-3 h-3 animate-spin text-primary-500 flex-shrink-0" />
        ) : ev.ok === false ? (
          <X className="w-3 h-3 text-red-500 flex-shrink-0" />
        ) : (
          <Check className="w-3 h-3 text-emerald-500 flex-shrink-0" />
        )}
        {task && !open && <span className="truncate font-normal text-aonyx-500">{task}</span>}
        <ChevronDown className={`ml-auto flex-shrink-0 w-3.5 h-3.5 text-aonyx-400 transition-transform ${open ? "rotate-180" : ""}`} />
      </button>
      {open && (task || ev.summary) && (
        <div className="px-2.5 pb-2 pt-1.5 text-xs space-y-2 border-t border-primary-500/15">
          {task && (
            <div>
              <span className="block text-[10px] uppercase tracking-wide text-aonyx-400 mb-0.5">{t("chat.task")}</span>
              <p className="text-aonyx-600 dark:text-aonyx-300 whitespace-pre-wrap break-words">{task}</p>
            </div>
          )}
          {ev.summary && (
            <div>
              <span className="block text-[10px] uppercase tracking-wide text-aonyx-400 mb-0.5">{t("chat.result")}</span>
              <p className="text-aonyx-600 dark:text-aonyx-300 whitespace-pre-wrap break-words">{resultText(ev.summary)}</p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default function Message({ role, content, events, error, streaming }) {
  const isUser = role === "user";
  const list = events || [];
  const delegations = list.filter((e) => e.name === "dispatch_agent");
  const otherTools = list.filter((e) => e.name !== "dispatch_agent");
  return (
    <div className={`flex flex-col gap-1.5 ${isUser ? "self-end items-end max-w-[80%]" : "self-start w-full max-w-[88%]"}`}>
      {isUser ? (
        <div
          className={`rounded-2xl rounded-tr-md px-4 py-2.5 text-sm select-text break-words whitespace-pre-wrap ${
            error
              ? "text-red-500 bg-red-500/10"
              : "bg-aonyx-200/60 dark:bg-aonyx-800/50 text-aonyx-900 dark:text-aonyx-50"
          }`}
        >
          {content}
        </div>
      ) : (
        <div
          className={`text-sm leading-relaxed select-text break-words ${
            error ? "text-red-500" : streaming ? "text-aonyx-500" : "text-aonyx-800 dark:text-aonyx-100"
          }`}
        >
          <Markdown>{content}</Markdown>
        </div>
      )}
      {delegations.map((ev, i) => (
        <Delegation key={i} ev={ev} />
      ))}
      {otherTools.length > 0 && (
        <div className="flex items-center gap-1.5 text-[11px] font-mono text-aonyx-500">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
          {otherTools.map((e) => e.name).join(", ")}
        </div>
      )}
    </div>
  );
}
