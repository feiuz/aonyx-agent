import { useEffect, useState } from "react";
import { Wrench, Sparkles, ShieldCheck, ShieldAlert, AlertTriangle } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useI18n } from "../context/LanguageContext";
import { useAgent } from "../context/AgentContext";
import * as agent from "../services/agentService";

// Skills & Tools view (Hermes-style top nav). Skills come from the /v1/info
// snapshot; tools from /v1/tools, grouped by safety class.
const CLASS_META = {
  safe: { color: "text-emerald-600 dark:text-emerald-400", ring: "border-emerald-500/30", Icon: ShieldCheck },
  caution: { color: "text-amber-600 dark:text-amber-400", ring: "border-amber-500/30", Icon: AlertTriangle },
  destructive: { color: "text-red-600 dark:text-red-400", ring: "border-red-500/30", Icon: ShieldAlert },
};

export default function SkillsTools() {
  const { t } = useI18n();
  const { info, status } = useAgent();
  const [tools, setTools] = useState([]);
  const [err, setErr] = useState(null);
  const [disabled, setDisabled] = useState(() => {
    try {
      return new Set(JSON.parse(localStorage.getItem("aonyx.toolsDisabled") || "[]"));
    } catch {
      return new Set();
    }
  });
  const skills = info?.skills || [];

  // Re-apply persisted toggles when the agent (re)connects, so the runner's
  // disabled set matches the UI after a restart.
  useEffect(() => {
    if (status !== "ok" || disabled.size === 0) return;
    disabled.forEach((name) => agent.toolEnabled(name, false).catch(() => {}));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  const toggle = (name) =>
    setDisabled((prev) => {
      const next = new Set(prev);
      const isOff = next.has(name);
      if (isOff) next.delete(name);
      else next.add(name);
      localStorage.setItem("aonyx.toolsDisabled", JSON.stringify([...next]));
      agent.toolEnabled(name, isOff).catch(() => {});
      return next;
    });

  useEffect(() => {
    let on = true;
    (async () => {
      try {
        const r = await agent.tools();
        if (on) setTools(Array.isArray(r) ? r : r?.tools || []);
      } catch (e) {
        if (on) setErr(String(e));
      }
    })();
    return () => {
      on = false;
    };
  }, [status]);

  const groups = [
    { key: "safe", label: t("skills.safe") },
    { key: "caution", label: t("skills.caution") },
    { key: "destructive", label: t("skills.destructive") },
  ];

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Wrench} title={t("nav.skills")} />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-3xl space-y-8">
          <section>
            <div className="flex items-center gap-2 mb-3">
              <Sparkles className="w-4 h-4 text-primary-500" />
              <h2 className="font-cond uppercase tracking-wide text-sm text-aonyx-700 dark:text-aonyx-300">{t("skills.skillsTitle")}</h2>
              <span className="text-xs text-aonyx-400">{skills.length}</span>
            </div>
            {skills.length === 0 ? (
              <p className="text-sm text-aonyx-500">{t("skills.noSkills")}</p>
            ) : (
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {skills.map((s) => (
                  <div key={s.id} className="rounded-xl border border-aonyx-200 dark:border-aonyx-800 p-3.5">
                    <div className="flex items-center gap-2">
                      <Sparkles className="w-3.5 h-3.5 text-primary-500 flex-shrink-0" />
                      <span className="font-medium text-aonyx-900 dark:text-aonyx-50 truncate">{s.id}</span>
                    </div>
                    {s.description && s.description !== s.id && (
                      <p className="text-xs text-aonyx-500 mt-1">{s.description}</p>
                    )}
                    {s.triggers?.length > 0 && (
                      <div className="flex flex-wrap gap-1 mt-2">
                        {s.triggers.map((tr, i) => (
                          <span key={i} className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-aonyx-100 dark:bg-aonyx-900 text-aonyx-500">{tr}</span>
                        ))}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </section>

          <section>
            <div className="flex items-center gap-2 mb-3">
              <Wrench className="w-4 h-4 text-primary-500" />
              <h2 className="font-cond uppercase tracking-wide text-sm text-aonyx-700 dark:text-aonyx-300">{t("skills.toolsTitle")}</h2>
              <span className="text-xs text-aonyx-400">{tools.length}</span>
            </div>
            {err && <p className="text-sm text-red-500">{err}</p>}
            {tools.length === 0 && !err && <p className="text-sm text-aonyx-500">{t("skills.noTools")}</p>}
            <div className="space-y-5">
              {groups.map((g) => {
                const list = tools.filter((x) => (x.class || "safe") === g.key);
                if (list.length === 0) return null;
                const meta = CLASS_META[g.key];
                const Icon = meta.Icon;
                return (
                  <div key={g.key}>
                    <div className={`flex items-center gap-1.5 mb-2 ${meta.color}`}>
                      <Icon className="w-3.5 h-3.5" />
                      <span className="text-xs font-medium uppercase tracking-wide">{g.label}</span>
                      <span className="text-xs opacity-60">{list.length}</span>
                    </div>
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                      {list.map((x) => {
                        const isOff = disabled.has(x.name);
                        return (
                          <div
                            key={x.name}
                            className={`rounded-lg border ${meta.ring} p-2.5 transition-opacity ${isOff ? "opacity-50" : ""}`}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <code className="text-xs font-mono text-aonyx-800 dark:text-aonyx-200 truncate">{x.name}</code>
                              <button
                                onClick={() => toggle(x.name)}
                                role="switch"
                                aria-checked={!isOff}
                                title={isOff ? t("tools.enable") : t("tools.disable")}
                                className={`relative w-8 h-[18px] rounded-full transition-colors flex-shrink-0 ${isOff ? "bg-aonyx-300 dark:bg-aonyx-700" : "bg-primary-600"}`}
                              >
                                <span
                                  className={`absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white shadow transition-all ${isOff ? "left-0.5" : "left-[14px]"}`}
                                />
                              </button>
                            </div>
                            {x.description && <p className="text-[11px] text-aonyx-500 mt-0.5 line-clamp-2">{x.description}</p>}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
