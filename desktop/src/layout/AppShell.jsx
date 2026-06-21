import { Outlet } from "react-router-dom";
import { Database } from "lucide-react";
import TitleBar from "./TitleBar";
import Sidebar from "./Sidebar";
import SignInModal from "../components/auth/SignInModal";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import pkg from "../../package.json";

// Hermes-style bottom status bar: agent health, memory mode, app version.
function StatusBar() {
  const { status, info } = useAgent();
  const { t } = useI18n();
  const label =
    status === "ok" ? t("status.ready") : status === "connecting" ? t("status.connecting") : t("status.offline");
  const dot = status === "ok" ? "bg-emerald-500" : status === "connecting" ? "bg-amber-500" : "bg-red-500";
  return (
    <div className="flex items-center gap-3 h-6 px-3 flex-shrink-0 border-t border-aonyx-200 dark:border-aonyx-800 bg-aonyx-100 dark:bg-aonyx-950 text-[11px] text-aonyx-500">
      <span className="flex items-center gap-1.5">
        <span className={`w-1.5 h-1.5 rounded-full ${dot}`} />
        {label}
      </span>
      {status === "ok" && info?.model && (
        <span className="hidden sm:inline font-mono truncate max-w-[200px]">{info.model}</span>
      )}
      <span className="hidden sm:flex items-center gap-1">
        <Database className="w-3 h-3" />
        {t("status.memoryLocal")}
      </span>
      <span className="flex-1" />
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
