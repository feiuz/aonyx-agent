import { Info, Github, Brain } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useI18n } from "../context/LanguageContext";
import pkg from "../../package.json";

export default function About() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Info} title={t("nav.about")} />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-2xl space-y-4">
          <div className="flex items-center gap-3">
            <Brain className="w-8 h-8 text-primary-500" strokeWidth={1.5} />
            <div>
              <p className="font-cond font-bold text-2xl text-aonyx-900 dark:text-aonyx-50 leading-none">Aonyx Agent</p>
              <p className="font-mono text-xs text-aonyx-500 mt-1">v{pkg.version} · MIT</p>
            </div>
          </div>
          <p className="text-sm text-aonyx-600 dark:text-aonyx-300 leading-relaxed">{t("about.tagline")}</p>
          <div className="flex flex-wrap gap-3 pt-1">
            <a
              href="https://github.com/feiuz/aonyx-agent"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1.5 text-sm text-primary-600 dark:text-primary-400 hover:underline"
            >
              <Github className="w-4 h-4" /> GitHub
            </a>
          </div>
        </div>
      </div>
    </div>
  );
}
