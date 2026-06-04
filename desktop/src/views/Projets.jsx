import { FolderOpen } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";

export default function Projets() {
  const { sessions, status } = useAgent();
  const { t } = useI18n();

  const counts = {};
  for (const s of sessions) {
    const p = s.project || t("projects.default");
    counts[p] = (counts[p] || 0) + 1;
  }
  const projects = Object.entries(counts).sort((a, b) => b[1] - a[1]);

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={FolderOpen} title={t("nav.projects")} subtitle={t("projects.subtitle")} />
      <div className="flex-1 overflow-y-auto p-6">
        {status !== "ok" ? (
          <p className="text-sm text-aonyx-500">{t("common.connect")}</p>
        ) : projects.length === 0 ? (
          <p className="text-sm text-aonyx-500">{t("projects.none")}</p>
        ) : (
          <ul className="max-w-2xl space-y-2">
            {projects.map(([p, n]) => (
              <li
                key={p}
                className="flex items-center justify-between rounded-lg border border-aonyx-200 dark:border-aonyx-800 px-4 py-3"
              >
                <span className="flex items-center gap-2 text-sm text-aonyx-800 dark:text-aonyx-200">
                  <FolderOpen className="w-4 h-4 text-aonyx-500" strokeWidth={1.75} />
                  {p}
                </span>
                <span className="text-xs font-mono text-aonyx-500">
                  {n} {t("projects.count")}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
