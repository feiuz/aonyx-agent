import { useEffect, useState } from "react";
import { Bot, Plus, Trash2, Save, X } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useI18n } from "../context/LanguageContext";
import { agentsList, agentsSave, agentsDelete } from "../services/agentsService";

// Agents view (ADR-017 / MA2): CRUD over user sub-agents (~/.aonyx/agents/).
// The architect delegates to these (plus the built-in presets) via dispatch_agent.
const BLANK = { id: "", name: "", description: "", model: "", tools: "", body: "" };
const inputCls =
  "w-full rounded-lg px-3 py-2 text-sm bg-white dark:bg-aonyx-950 border border-aonyx-300 dark:border-aonyx-700 focus:outline-none focus:border-primary-500 select-text";

function Field({ label, children }) {
  return (
    <label className="block">
      <span className="block text-xs uppercase tracking-wide text-aonyx-500 mb-1">{label}</span>
      {children}
    </label>
  );
}

export default function Agents() {
  const { t } = useI18n();
  const [agents, setAgents] = useState([]);
  const [sel, setSel] = useState(null);
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);

  const load = async () => {
    const r = await agentsList();
    setAgents(r?.agents || []);
  };
  useEffect(() => {
    load();
  }, []);

  const edit = (a) => {
    setMsg("");
    setSel({ ...BLANK, ...a, model: a.model || "", tools: (a.tools || []).join(", ") });
  };
  const create = () => {
    setMsg("");
    setSel({ ...BLANK });
  };

  const save = async () => {
    if (!sel.name.trim()) return setMsg(t("agents.name"));
    setBusy(true);
    setMsg("");
    const payload = {
      id: sel.id || "",
      name: sel.name.trim(),
      description: sel.description.trim(),
      model: sel.model.trim() || null,
      tools: sel.tools.split(",").map((s) => s.trim()).filter(Boolean),
      body: sel.body,
    };
    try {
      await agentsSave(payload);
      setSel(null);
      await load();
      setMsg(t("agents.saved"));
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!sel?.id) return setSel(null);
    setBusy(true);
    try {
      await agentsDelete(sel.id);
      setSel(null);
      await load();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Bot} title={t("nav.agents")} />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-3xl space-y-5">
          <p className="text-sm text-aonyx-500">{t("agents.subtitle")}</p>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            {agents.map((a) => (
              <button
                key={a.id}
                onClick={() => edit(a)}
                className={`text-left rounded-xl border p-3.5 transition-colors ${
                  sel?.id === a.id
                    ? "border-primary-600 ring-1 ring-primary-600"
                    : "border-aonyx-200 dark:border-aonyx-800 hover:border-aonyx-300 dark:hover:border-aonyx-700"
                }`}
              >
                <div className="flex items-center gap-2">
                  <Bot className="w-4 h-4 text-aonyx-500" />
                  <span className="font-medium text-aonyx-900 dark:text-aonyx-50">{a.name}</span>
                </div>
                {a.description && <p className="text-xs text-aonyx-500 mt-1 line-clamp-2">{a.description}</p>}
                {a.tools?.length > 0 && (
                  <p className="text-[10px] font-mono text-aonyx-400 mt-1.5 truncate">{a.tools.join(", ")}</p>
                )}
              </button>
            ))}
            <button
              onClick={create}
              className="flex items-center justify-center gap-2 rounded-xl border border-dashed border-aonyx-300 dark:border-aonyx-700 p-3.5 text-sm text-aonyx-500 hover:border-primary-500 hover:text-primary-600 transition-colors"
            >
              <Plus className="w-4 h-4" /> {t("agents.new")}
            </button>
          </div>

          {agents.length === 0 && !sel && <p className="text-sm text-aonyx-500">{t("agents.none")}</p>}

          {sel && (
            <div className="rounded-xl border border-aonyx-200 dark:border-aonyx-800 p-5 space-y-4 bg-white dark:bg-aonyx-950">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <Field label={t("agents.name")}>
                  <input value={sel.name} onChange={(e) => setSel({ ...sel, name: e.target.value })} className={inputCls} placeholder="Tester" />
                </Field>
                <Field label={t("agents.model")}>
                  <input value={sel.model} onChange={(e) => setSel({ ...sel, model: e.target.value })} className={`${inputCls} font-mono`} placeholder="(inherit)" spellCheck={false} />
                </Field>
              </div>
              <Field label={t("agents.description")}>
                <input value={sel.description} onChange={(e) => setSel({ ...sel, description: e.target.value })} className={inputCls} />
              </Field>
              <Field label={t("agents.tools")}>
                <input value={sel.tools} onChange={(e) => setSel({ ...sel, tools: e.target.value })} className={`${inputCls} font-mono`} placeholder="fs_read, fs_write, bash, git_*" spellCheck={false} />
              </Field>
              <Field label={t("agents.prompt")}>
                <textarea value={sel.body} onChange={(e) => setSel({ ...sel, body: e.target.value })} rows={5} className={`${inputCls} resize-y`} placeholder="You are…" />
              </Field>
              <div className="flex flex-wrap items-center gap-3 pt-1">
                <button onClick={save} disabled={busy} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-primary-600 hover:bg-primary-700 text-white font-medium disabled:opacity-50">
                  <Save className="w-4 h-4" /> {t("agents.save")}
                </button>
                {sel.id && (
                  <button onClick={remove} disabled={busy} className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border border-red-500/40 text-red-600 dark:text-red-400 hover:bg-red-500/10">
                    <Trash2 className="w-4 h-4" /> {t("agents.delete")}
                  </button>
                )}
                <button onClick={() => setSel(null)} className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border border-aonyx-300 dark:border-aonyx-700 hover:bg-aonyx-100 dark:hover:bg-aonyx-900">
                  <X className="w-4 h-4" /> {t("agents.cancel")}
                </button>
                {msg && <span className="text-sm text-aonyx-500">{msg}</span>}
              </div>
            </div>
          )}

          <p className="text-xs text-aonyx-400 border-t border-aonyx-200 dark:border-aonyx-800 pt-3">{t("agents.builtin")}</p>
        </div>
      </div>
    </div>
  );
}
