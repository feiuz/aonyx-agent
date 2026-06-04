import { BarChart3, MessageSquare, Hash, Cpu, Box } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import StatCard from "../components/ui/StatCard";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";

export default function Stats() {
  const { sessions, info, status } = useAgent();
  const { t } = useI18n();
  const totalTurns = sessions.reduce((n, s) => n + (s.turns || 0), 0);

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={BarChart3} title={t("nav.stats")} />
      <div className="flex-1 overflow-y-auto p-6">
        {status !== "ok" ? (
          <p className="text-sm text-aonyx-500">{t("common.connect")}</p>
        ) : (
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 max-w-3xl">
            <StatCard icon={MessageSquare} label={t("stats.conversations")} value={sessions.length} />
            <StatCard icon={Hash} label={t("stats.turns")} value={totalTurns} />
            <StatCard icon={Cpu} label={t("stats.provider")} value={info?.provider || "—"} small />
            <StatCard icon={Box} label={t("stats.model")} value={info?.model || "—"} small />
          </div>
        )}
      </div>
    </div>
  );
}
