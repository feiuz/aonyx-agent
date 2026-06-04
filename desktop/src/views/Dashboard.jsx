import { Link } from "react-router-dom";
import {
  LayoutDashboard,
  MessageSquare,
  Activity,
  Database,
  Settings as SettingsIcon,
  Cpu,
  Box,
  Hash,
} from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import StatCard from "../components/ui/StatCard";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";

const QUICK = [
  { to: "/chat", icon: MessageSquare, key: "nav.chat" },
  { to: "/memory-health", icon: Activity, key: "nav.memory" },
  { to: "/kg", icon: Database, key: "nav.kg" },
  { to: "/settings", icon: SettingsIcon, key: "nav.settings" },
];

export default function Dashboard() {
  const { status, info, sessions } = useAgent();
  const { t } = useI18n();
  const totalTurns = sessions.reduce((n, s) => n + (s.turns || 0), 0);

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        icon={LayoutDashboard}
        title={t("nav.dashboard")}
        subtitle={status === "ok" && info ? `${info.provider} · ${info.model}` : t("status.offline")}
      />
      <div className="flex-1 overflow-y-auto p-6 space-y-6">
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 max-w-3xl">
          <StatCard icon={MessageSquare} label={t("stats.conversations")} value={sessions.length} />
          <StatCard icon={Hash} label={t("stats.turns")} value={totalTurns} />
          <StatCard icon={Cpu} label={t("stats.provider")} value={info?.provider || "—"} small />
          <StatCard icon={Box} label={t("stats.model")} value={info?.model || "—"} small />
        </div>
        <div className="flex flex-wrap gap-3 max-w-3xl">
          {QUICK.map(({ to, icon: Icon, key }) => (
            <Link
              key={to}
              to={to}
              className="flex items-center gap-2 px-4 py-2 rounded-lg border border-aonyx-200 dark:border-aonyx-800 hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/50 text-sm text-aonyx-700 dark:text-aonyx-200"
            >
              <Icon className="w-4 h-4 text-aonyx-500" strokeWidth={1.75} /> {t(key)}
            </Link>
          ))}
        </div>
      </div>
    </div>
  );
}
