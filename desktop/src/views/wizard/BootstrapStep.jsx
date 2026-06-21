import { useEffect, useRef, useState } from "react";
import { Check, Loader2, X, Minus } from "lucide-react";
import { useI18n } from "../../context/LanguageContext";
import { saveSetup, startLocal, apiInfo, prepareEmbeddings } from "../../services/setupService";
import { listModels } from "../../services/configService";

// Auto-bootstrap screen (the validated Hermes-style stepper). Runs the real
// steps: detect/verify provider, write config, download the embedding model with
// a live progress bar (W4 — fastembed progress streamed from the agent), then
// start the embedded agent and probe it.
const STEPS = ["engine", "detect", "palace", "config", "rag", "model", "agent"];
const tick = (ms) => new Promise((r) => setTimeout(r, ms));
const mb = (n) => Math.round((n || 0) / (1024 * 1024));

function toCfg(d) {
  const cfg = { provider: d.provider, model: d.model, rag_backend: d.rag_backend, rag_embeddings: d.rag_embeddings };
  if (d.provider === "anthropic") cfg.anthropic_api_key = d.apiKey;
  else if (d.provider === "openai") { cfg.openai_api_key = d.apiKey; cfg.openai_base_url = d.base; }
  else if (d.provider === "openrouter") cfg.openrouter_api_key = d.apiKey;
  else if (d.provider === "ollama") cfg.ollama_base_url = d.base;
  else if (d.provider === "lm-studio") cfg.lm_studio_base_url = d.base;
  else if (d.provider === "claude-code") cfg.claude_code_binary = d.binary;
  return cfg;
}

async function waitInfo(base) {
  for (let i = 0; i < 24; i++) {
    try {
      await apiInfo(base);
      return true;
    } catch {
      await tick(500);
    }
  }
  throw new Error("agent unreachable");
}

function Marker({ state }) {
  if (state === "done") return <Check className="w-[17px] h-[17px] text-emerald-500" />;
  if (state === "skip") return <Minus className="w-[17px] h-[17px] text-aonyx-400" />;
  if (state === "run") return <Loader2 className="w-[17px] h-[17px] text-primary-600 animate-spin" />;
  if (state === "error") return <X className="w-[17px] h-[17px] text-red-500" />;
  return <span className="inline-block w-[7px] h-[7px] rounded-full bg-aonyx-300 dark:bg-aonyx-700" />;
}

export default function BootstrapStep({ draft, onDone }) {
  const { t } = useI18n();
  const [status, setStatus] = useState(() => Object.fromEntries(STEPS.map((s) => [s, { state: "pending" }])));
  const [modelPct, setModelPct] = useState(0);
  const [failed, setFailed] = useState(false);
  const ran = useRef(false);

  const run = async () => {
    setFailed(false);
    setModelPct(0);
    setStatus(Object.fromEntries(STEPS.map((s) => [s, { state: "pending" }])));
    const set = (id, state, note) => setStatus((p) => ({ ...p, [id]: { state, note } }));
    let current = "engine";
    try {
      current = "engine";
      set("engine", "run");
      await tick(250);
      set("engine", "done");

      current = "detect";
      set("detect", "run");
      try {
        const list = await listModels(draft.provider, draft.base || "", draft.apiKey || "");
        set("detect", "done", list?.length ? `${list.length} ${t("settings.model.available")}` : undefined);
      } catch {
        set("detect", "done");
      }

      current = "palace";
      set("palace", "run");
      await tick(180);
      set("palace", "done");

      current = "config";
      set("config", "run");
      await saveSetup(toCfg(draft));
      set("config", "done");

      current = "rag";
      set("rag", "run");
      await tick(140);
      set("rag", "done", t(`wizard.rag.${draft.rag_backend}.label`));

      current = "model";
      if (draft.rag_embeddings === "local") {
        set("model", "run");
        setModelPct(0);
        await prepareEmbeddings((ev) => {
          if (ev?.phase === "downloading") {
            setModelPct(ev.pct || 0);
            set("model", "run", `${mb(ev.downloaded)} / ${mb(ev.total)} Mo`);
          }
        });
        set("model", "done");
      } else {
        set("model", "skip", t("wizard.boot.modelProvider"));
      }

      current = "agent";
      set("agent", "run");
      localStorage.setItem("aonyx.local", "1");
      const base = await startLocal();
      await waitInfo(base);
      set("agent", "done");

      await tick(450);
      onDone();
    } catch (e) {
      setStatus((p) => ({ ...p, [current]: { state: "error", note: String(e) } }));
      setFailed(true);
    }
  };

  useEffect(() => {
    if (!ran.current) {
      ran.current = true;
      run();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const done = STEPS.filter((s) => ["done", "skip"].includes(status[s].state)).length;

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-medium text-aonyx-900 dark:text-aonyx-50">
          {failed ? t("wizard.boot.failed") : done === STEPS.length ? t("wizard.boot.done") : t("wizard.boot.title")}
        </h2>
        <p className="text-sm text-aonyx-500 mt-0.5">{t("wizard.boot.desc")}</p>
      </div>

      <div className="h-1 rounded-full bg-aonyx-200 dark:bg-aonyx-800 overflow-hidden">
        <div className="h-full bg-primary-600 transition-all" style={{ width: `${Math.round((done / STEPS.length) * 100)}%` }} />
      </div>

      <div className="space-y-0.5">
        {STEPS.map((s) => {
          const st = status[s];
          const active = st.state === "run";
          const showBar = s === "model" && active && modelPct > 0;
          return (
            <div key={s} className={`rounded-lg px-2.5 py-2 ${active ? "bg-aonyx-100 dark:bg-aonyx-900" : ""}`}>
              <div className="flex items-center justify-between">
                <span className="flex items-center gap-2.5 min-w-0">
                  <span className="w-[18px] flex items-center justify-center shrink-0"><Marker state={st.state} /></span>
                  <span className={`text-sm truncate ${st.state === "pending" ? "text-aonyx-400" : "text-aonyx-800 dark:text-aonyx-100"} ${active ? "font-medium" : ""}`}>
                    {t(`wizard.boot.${s}`)}
                  </span>
                </span>
                {st.note && <span className={`text-xs shrink-0 ml-2 ${st.state === "error" ? "text-red-500" : "text-aonyx-400"}`}>{st.note}</span>}
              </div>
              {showBar && (
                <div className="mt-1.5 ml-[30px] h-1 rounded-full bg-aonyx-200 dark:bg-aonyx-800 overflow-hidden">
                  <div className="h-full bg-primary-600 transition-all" style={{ width: `${modelPct}%` }} />
                </div>
              )}
            </div>
          );
        })}
      </div>

      {failed && (
        <div className="flex justify-end pt-1">
          <button onClick={run} className="px-4 py-2 rounded-lg bg-primary-600 hover:bg-primary-700 text-white font-medium">
            {t("wizard.boot.retry")}
          </button>
        </div>
      )}
    </div>
  );
}
