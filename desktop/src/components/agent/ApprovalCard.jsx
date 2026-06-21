import { ShieldAlert, Check, X } from "lucide-react";
import { useI18n } from "../../context/LanguageContext";

// Compact, readable preview of a tool's arguments (path/command first, long
// values like file content truncated).
function preview(args) {
  if (!args || typeof args !== "object") return String(args ?? "");
  return Object.entries(args)
    .map(([k, v]) => {
      let s = typeof v === "string" ? v : JSON.stringify(v);
      if (s.length > 300) s = s.slice(0, 300) + "…";
      return `${k}: ${s}`;
    })
    .join("\n");
}

// Interactive approval (Hermes-style): a destructive tool call is paused; the
// user approves or refuses. Resolves via api_approve(id, approved).
export default function ApprovalCard({ req, onDecide }) {
  const { t } = useI18n();
  return (
    <div className="self-start w-full max-w-[88%] rounded-xl border border-amber-500/40 bg-amber-50/60 dark:bg-amber-950/20 p-3.5">
      <div className="flex items-center gap-2 mb-1.5">
        <ShieldAlert className="w-4 h-4 text-amber-600 dark:text-amber-400 flex-shrink-0" />
        <span className="text-sm font-medium text-amber-800 dark:text-amber-200">{t("approval.title")}</span>
        <code className="text-xs font-mono px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-700 dark:text-amber-300">{req.name}</code>
      </div>
      <p className="text-xs text-aonyx-500 mb-2">{t("approval.subtitle")}</p>
      <pre className="text-[11px] font-mono leading-relaxed text-aonyx-700 dark:text-aonyx-300 bg-white/60 dark:bg-black/20 rounded-lg p-2.5 max-h-40 overflow-auto whitespace-pre-wrap break-all mb-3">
        {preview(req.args)}
      </pre>
      <div className="flex items-center gap-2">
        <button
          onClick={() => onDecide(req.id, true)}
          className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-700 text-white text-sm font-medium"
        >
          <Check className="w-4 h-4" /> {t("approval.approve")}
        </button>
        <button
          onClick={() => onDecide(req.id, false)}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-red-500/40 text-red-600 dark:text-red-400 hover:bg-red-500/10 text-sm"
        >
          <X className="w-4 h-4" /> {t("approval.deny")}
        </button>
      </div>
    </div>
  );
}
