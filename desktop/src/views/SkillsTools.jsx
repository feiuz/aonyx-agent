import { useEffect, useMemo, useState } from "react";
import { Wrench, Sparkles, ShieldCheck, ShieldAlert, AlertTriangle, Search } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useI18n } from "../context/LanguageContext";
import { useAgent } from "../context/AgentContext";
import * as agent from "../services/agentService";

// Skills & Tools view. Skills (built-in + user) come from /v1/skills grouped by
// category; tools from /v1/tools grouped by safety class. Both can be toggled
// on/off (persisted in localStorage, re-applied to the runner on reconnect).
const CLASS_META = {
  safe: { color: "text-emerald-600 dark:text-emerald-400", ring: "border-emerald-500/30", Icon: ShieldCheck },
  caution: { color: "text-amber-600 dark:text-amber-400", ring: "border-amber-500/30", Icon: AlertTriangle },
  destructive: { color: "text-red-600 dark:text-red-400", ring: "border-red-500/30", Icon: ShieldAlert },
};

const loadSet = (key) => {
  try {
    return new Set(JSON.parse(localStorage.getItem(key) || "[]"));
  } catch {
    return new Set();
  }
};

function Switch({ on, onClick, title }) {
  return (
    <button
      onClick={onClick}
      role="switch"
      aria-checked={on}
      title={title}
      className={`relative w-8 h-[18px] rounded-full transition-colors flex-shrink-0 ${on ? "bg-primary-600" : "bg-aonyx-300 dark:bg-aonyx-700"}`}
    >
      <span className={`absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white shadow transition-all ${on ? "left-[14px]" : "left-0.5"}`} />
    </button>
  );
}

export default function SkillsTools() {
  const { t } = useI18n();
  const { status } = useAgent();
  const [tools, setTools] = useState([]);
  const [skills, setSkills] = useState([]);
  const [err, setErr] = useState(null);
  const [query, setQuery] = useState("");
  const [toolsOff, setToolsOff] = useState(() => loadSet("aonyx.toolsDisabled"));
  const [skillsOff, setSkillsOff] = useState(() => loadSet("aonyx.skillsDisabled"));

  useEffect(() => {
    let on = true;
    (async () => {
      try {
        const [tr, sr] = await Promise.all([agent.tools(), agent.skills()]);
        if (!on) return;
        setTools(Array.isArray(tr) ? tr : tr?.tools || []);
        setSkills(Array.isArray(sr) ? sr : sr?.skills || []);
      } catch (e) {
        if (on) setErr(String(e));
      }
    })();
    return () => {
      on = false;
    };
  }, [status]);

  // Re-apply persisted toggles on (re)connect so the runner matches the UI.
  useEffect(() => {
    if (status !== "ok") return;
    toolsOff.forEach((n) => agent.toolEnabled(n, false).catch(() => {}));
    skillsOff.forEach((id) => agent.skillEnabled(id, false).catch(() => {}));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  const toggleTool = (name) =>
    setToolsOff((prev) => {
      const next = new Set(prev);
      const isOff = next.has(name);
      if (isOff) next.delete(name);
      else next.add(name);
      localStorage.setItem("aonyx.toolsDisabled", JSON.stringify([...next]));
      agent.toolEnabled(name, isOff).catch(() => {});
      return next;
    });

  const toggleSkill = (id) =>
    setSkillsOff((prev) => {
      const next = new Set(prev);
      const isOff = next.has(id);
      if (isOff) next.delete(id);
      else next.add(id);
      localStorage.setItem("aonyx.skillsDisabled", JSON.stringify([...next]));
      agent.skillEnabled(id, isOff).catch(() => {});
      return next;
    });

  const q = query.trim().toLowerCase();
  const matchSkill = (s) =>
    !q || `${s.id} ${s.name} ${s.description} ${(s.tags || []).join(" ")} ${s.category || ""}`.toLowerCase().includes(q);
  const matchTool = (x) => !q || `${x.name} ${x.description}`.toLowerCase().includes(q);

  const skillCats = useMemo(() => {
    const map = new Map();
    skills.filter(matchSkill).forEach((s) => {
      const cat = s.category || "general";
      if (!map.has(cat)) map.set(cat, []);
      map.get(cat).push(s);
    });
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [skills, q]);

  const toolGroups = [
    { key: "safe", label: t("skills.safe") },
    { key: "caution", label: t("skills.caution") },
    { key: "destructive", label: t("skills.destructive") },
  ];

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Wrench} title={t("nav.skills")} />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-3xl space-y-8">
          <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-aonyx-100 dark:bg-aonyx-900/50 border border-aonyx-200 dark:border-aonyx-800">
            <Search className="w-4 h-4 text-aonyx-400 flex-shrink-0" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("skills.search")}
              className="flex-1 min-w-0 bg-transparent text-sm focus:outline-none placeholder:text-aonyx-400"
            />
          </div>

          <section>
            <div className="flex items-center gap-2 mb-3">
              <Sparkles className="w-4 h-4 text-primary-500" />
              <h2 className="font-cond uppercase tracking-wide text-sm text-aonyx-700 dark:text-aonyx-300">{t("skills.skillsTitle")}</h2>
              <span className="text-xs text-aonyx-400">{skills.length}</span>
            </div>
            {skills.length === 0 ? (
              <p className="text-sm text-aonyx-500">{t("skills.noSkills")}</p>
            ) : (
              <div className="space-y-5">
                {skillCats.map(([cat, list]) => (
                  <div key={cat}>
                    <div className="flex items-center gap-1.5 mb-2 text-primary-700 dark:text-primary-400">
                      <span className="text-xs font-medium uppercase tracking-wide">{cat}</span>
                      <span className="text-xs opacity-60">{list.length}</span>
                    </div>
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                      {list.map((s) => {
                        const isOff = skillsOff.has(s.id);
                        return (
                          <div
                            key={s.id}
                            className={`rounded-xl border border-aonyx-200 dark:border-aonyx-800 p-3 transition-opacity ${isOff ? "opacity-50" : ""}`}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <span className="flex items-center gap-1.5 min-w-0">
                                <Sparkles className="w-3.5 h-3.5 text-primary-500 flex-shrink-0" />
                                <span className="font-medium text-sm text-aonyx-900 dark:text-aonyx-50 truncate">{s.name || s.id}</span>
                              </span>
                              <Switch on={!isOff} onClick={() => toggleSkill(s.id)} title={isOff ? t("tools.enable") : t("tools.disable")} />
                            </div>
                            {s.description && s.description !== s.name && (
                              <p className="text-xs text-aonyx-500 mt-1 line-clamp-2">{s.description}</p>
                            )}
                            {s.tags?.length > 0 && (
                              <div className="flex flex-wrap gap-1 mt-1.5">
                                {s.tags.slice(0, 4).map((tg, i) => (
                                  <span key={i} className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-aonyx-100 dark:bg-aonyx-900 text-aonyx-500">{tg}</span>
                                ))}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
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
              {toolGroups.map((g) => {
                const list = tools.filter((x) => (x.class || "safe") === g.key).filter(matchTool);
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
                        const isOff = toolsOff.has(x.name);
                        return (
                          <div
                            key={x.name}
                            className={`rounded-lg border ${meta.ring} p-2.5 transition-opacity ${isOff ? "opacity-50" : ""}`}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <code className="text-xs font-mono text-aonyx-800 dark:text-aonyx-200 truncate">{x.name}</code>
                              <Switch on={!isOff} onClick={() => toggleTool(x.name)} title={isOff ? t("tools.enable") : t("tools.disable")} />
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
