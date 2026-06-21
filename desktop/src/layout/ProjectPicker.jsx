import { useState } from "react";
import { FolderOpen, Plus, Check } from "lucide-react";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";

// Active RAG project for new conversations. The memory palace is scoped per
// project; a new project name is created on first ingest/use.
export default function ProjectPicker() {
  const { project, setProject, projects } = useAgent();
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState("");

  const label = project || t("project.default");
  const pick = (p) => {
    setProject(p);
    setOpen(false);
  };
  const createNew = () => {
    const name = creating.trim();
    if (!name) return;
    setProject(name);
    setCreating("");
    setOpen(false);
  };

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        title={label}
        className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-aonyx-600 dark:text-aonyx-400 hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/50 transition-colors"
      >
        <FolderOpen className="w-3.5 h-3.5 text-primary-500 flex-shrink-0" />
        <span className="text-[10px] uppercase tracking-wide text-aonyx-400">{t("project.label")}</span>
        <span className="font-medium text-xs truncate flex-1 text-left">{label}</span>
      </button>
      {open && (
        <div className="absolute z-20 left-0 right-0 mt-1 rounded-lg border border-aonyx-200 dark:border-aonyx-800 bg-white dark:bg-aonyx-950 shadow-lg p-1 max-h-64 overflow-y-auto">
          <button
            onClick={() => pick("")}
            className="w-full flex items-center gap-2 px-2 py-1.5 rounded text-xs hover:bg-aonyx-100 dark:hover:bg-aonyx-900"
          >
            {project === "" ? <Check className="w-3 h-3 text-primary-500" /> : <span className="w-3" />}
            <span className="truncate flex-1 text-left">{t("project.default")}</span>
          </button>
          {projects.map((p) => (
            <button
              key={p.project}
              onClick={() => pick(p.project)}
              className="w-full flex items-center gap-2 px-2 py-1.5 rounded text-xs hover:bg-aonyx-100 dark:hover:bg-aonyx-900"
            >
              {project === p.project ? <Check className="w-3 h-3 text-primary-500" /> : <span className="w-3" />}
              <span className="truncate flex-1 text-left">{p.project}</span>
              <span className="text-[10px] text-aonyx-400">{p.chunks}</span>
            </button>
          ))}
          <div className="flex items-center gap-1 mt-1 pt-1 border-t border-aonyx-200 dark:border-aonyx-800">
            <input
              value={creating}
              onChange={(e) => setCreating(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && createNew()}
              placeholder={t("project.new")}
              className="flex-1 min-w-0 bg-transparent text-xs px-2 py-1 focus:outline-none placeholder:text-aonyx-400"
            />
            <button onClick={createNew} className="p-1 rounded hover:bg-aonyx-100 dark:hover:bg-aonyx-900">
              <Plus className="w-3.5 h-3.5 text-primary-500" />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
