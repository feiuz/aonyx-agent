import { useEffect, useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import { Database, Bot, Clock } from "lucide-react";
import TitleBar from "./TitleBar";
import Sidebar from "./Sidebar";
import SignInModal from "../components/auth/SignInModal";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import pkg from "../../package.json";

const fmtTok = (n) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`);

// Hermes-style bottom status bar: agent health, the live model, quick links to
// Agents and Memory, the context gauge, the session clock, and the app version.
function StatusBar() {
  const { status, info, usage } = useAgent();
  const { t } = useI18n();
  const navigate = useNavigate();
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    const start = Date.now();
    const id = setInterval(() => setElapsed(Math.floor((Date.now() - start) / 1000)), 1000);
    return () => clearInterval(id);
  }, []);
  const clock = `${String(Math.floor(elapsed / 60)).padStart(2, "0")}:${String(elapsed % 60).padStart(2, "0")}`;
  const label =
    status === "ok" ? t("status.ready") : status === "connecting" ? t("status.connecting") : t("status.offline");
  const dot = status === "ok" ? "bg-emerald-500" : status === "connecting" ? "bg-amber-500" : "bg-red-500";
  const cell = "flex items-center gap-1 hover:text-aonyx-700 dark:hover:text-aonyx-300 transition-colors";
  const pct = usage?.max ? Math.min(100, Math.round((usage.tokens / usage.max) * 100)) : 0;
  const barColor = pct > 85 ? "bg-red-500" : pct > 60 ? "bg-amber-500" : "bg-emerald-500";
  return (
    <div className="flex items-center gap-3 h-6 px-3 flex-shrink-0 border-t border-aonyx-200 dark:border-aonyx-800 bg-aonyx-100 dark:bg-aonyx-950 text-[11px] text-aonyx-500">
      <button onClick={() => navigate("/settings")} className={cell} title={info ? `${info.provider} · ${info.model}` : ""}>
        <span className={`w-1.5 h-1.5 rounded-full ${dot}`} />
        {label}
      </button>
      {status === "ok" && info?.model && (
        <span className="hidden md:inline font-mono truncate max-w-[160px]">{info.model}</span>
      )}
      <button onClick={() => navigate("/agents")} className={`hidden sm:flex ${cell}`}>
        <Bot className="w-3 h-3" />
        {t("nav.agents")}
      </button>
      <button onClick={() => navigate("/memory")} className={`hidden sm:flex ${cell}`}>
        <Database className="w-3 h-3" />
        {t("status.memoryLocal")}
      </button>
      <span className="flex-1" />
      {usage?.tokens > 0 && (
        <span className="hidden lg:flex items-center gap-1.5" title={t("status.context")}>
          <span className="font-mono">
            {fmtTok(usage.tokens)}/{fmtTok(usage.max)}
          </span>
          <span className="w-16 h-1.5 rounded-full bg-aonyx-200 dark:bg-aonyx-800 overflow-hidden">
            <span className={`block h-full rounded-full ${barColor}`} style={{ width: `${pct}%` }} />
          </span>
          <span className="font-mono">{pct}%</span>
        </span>
      )}
      <span className="flex items-center gap-1 font-mono" title={t("status.session")}>
        <Clock className="w-3 h-3" />
        {clock}
      </span>
      <span className="font-mono">v{pkg.version}</span>
    </div>
  );
}

export default function AppShell() {
  return (
    <div className="flex flex-col h-screen overflow-hidden">
      <TitleBar />
      <div className="flex flex-1 min-h-0">
        <Sidebar />
        <main className="flex-1 min-w-0 overflow-hidden bg-aonyx-50 dark:bg-aonyx-900">
          <Outlet />
        </main>
      </div>
      <StatusBar />
      <SignInModal />
    </div>
  );
}
