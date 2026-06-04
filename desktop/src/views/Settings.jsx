import { useCallback, useEffect, useState } from "react";
import { Settings as SettingsIcon, RefreshCw } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import { readProviderConfig, saveProviderConfig, listModels } from "../services/configService";

const PROVIDERS = [
  { id: "anthropic", label: "Anthropic — Claude" },
  { id: "openai", label: "OpenAI" },
  { id: "openrouter", label: "OpenRouter" },
  { id: "ollama", label: "Ollama — local" },
  { id: "lm-studio", label: "LM Studio — local" },
  { id: "claude-code", label: "Claude Code — session" },
];
const DEFAULT_BASE = {
  openai: "https://api.openai.com",
  ollama: "http://localhost:11434",
  "lm-studio": "http://localhost:1234",
};
const NEEDS_KEY = new Set(["anthropic", "openai", "openrouter"]);
const HAS_BASE = new Set(["openai", "ollama", "lm-studio"]);
const CUSTOM = "__custom__";

const conn = {
  get local() { return localStorage.getItem("aonyx.local") !== "0"; },
  set local(v) { localStorage.setItem("aonyx.local", v ? "1" : "0"); },
  get url() { return localStorage.getItem("aonyx.apiUrl") || "http://127.0.0.1:8788"; },
  set url(v) { localStorage.setItem("aonyx.apiUrl", v); },
  get token() { return localStorage.getItem("aonyx.token") || ""; },
  set token(v) { localStorage.setItem("aonyx.token", v); },
};

const inputCls =
  "w-full rounded-lg px-3 py-2 text-sm font-mono bg-white dark:bg-aonyx-950 border border-aonyx-300 dark:border-aonyx-700 focus:outline-none focus:border-primary-500 select-text disabled:opacity-50";

function Field({ label, children }) {
  return (
    <label className="block">
      <span className="block text-xs uppercase tracking-wide text-aonyx-500 mb-1">{label}</span>
      {children}
    </label>
  );
}

export default function Settings() {
  const { connect } = useAgent();
  const { t } = useI18n();

  const [provider, setProvider] = useState("anthropic");
  const [model, setModel] = useState("");
  const [customModel, setCustomModel] = useState("");
  const [models, setModels] = useState([]);
  const [modelNote, setModelNote] = useState("");
  const [loadingModels, setLoadingModels] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [base, setBase] = useState("");
  const [binary, setBinary] = useState("");

  const [local, setLocal] = useState(conn.local);
  const [apiUrl, setApiUrl] = useState(conn.url);
  const [token, setToken] = useState(conn.token);

  const [msg, setMsg] = useState("");
  const [saving, setSaving] = useState(false);

  const loadModels = useCallback(async (prov, current, keyVal, baseVal) => {
    setLoadingModels(true);
    setModelNote("chargement des modèles…");
    try {
      const list = (await listModels(prov, baseVal || DEFAULT_BASE[prov] || "", keyVal || "")) || [];
      const finalList = current && !list.includes(current) ? [current, ...list] : list;
      setModels(finalList);
      if (!current && finalList.length) setModel(finalList[0]);
      setModelNote(
        list.length
          ? `${list.length} modèle${list.length > 1 ? "s" : ""} disponible${list.length > 1 ? "s" : ""}`
          : "aucun modèle retourné — utilise Custom",
      );
    } catch (e) {
      const m = String(e);
      setModels(current ? [current] : []);
      setModelNote(
        m.includes("API_KEY_REQUIRED")
          ? "🔑 Clé API requise — saisis-la puis ↻"
          : "fetch impossible : " + m,
      );
    } finally {
      setLoadingModels(false);
    }
  }, []);

  useEffect(() => {
    (async () => {
      const c = await readProviderConfig();
      const p = c.provider || "anthropic";
      const k = c.anthropic_api_key || c.openai_api_key || c.openrouter_api_key || "";
      const b = c.openai_base_url || c.ollama_base_url || c.lm_studio_base_url || "";
      setProvider(p);
      setModel(c.model || "");
      setApiKey(k);
      setBase(b);
      setBinary(c.claude_code_binary || "");
      loadModels(p, c.model || null, k, b);
    })();
  }, [loadModels]);

  const onProviderChange = (p) => {
    setProvider(p);
    setModel("");
    loadModels(p, null, apiKey, base);
  };

  const selectedModel = () => (model === CUSTOM ? customModel.trim() : model);

  const save = async () => {
    const m = selectedModel();
    if (!m) return setMsg("Choisis un modèle (ou saisis un id custom).");
    if (NEEDS_KEY.has(provider) && !apiKey.trim()) return setMsg("Ce fournisseur requiert une clé API.");

    const cfg = { provider, model: m };
    if (provider === "anthropic") cfg.anthropic_api_key = apiKey;
    else if (provider === "openai") { cfg.openai_api_key = apiKey; cfg.openai_base_url = base.trim(); }
    else if (provider === "openrouter") cfg.openrouter_api_key = apiKey;
    else if (provider === "ollama") cfg.ollama_base_url = base.trim();
    else if (provider === "lm-studio") cfg.lm_studio_base_url = base.trim();
    else if (provider === "claude-code") cfg.claude_code_binary = binary.trim();

    conn.local = local;
    conn.url = apiUrl.trim() || "http://127.0.0.1:8788";
    conn.token = token;

    setSaving(true);
    setMsg("Enregistrement + reconnexion…");
    try {
      await saveProviderConfig(cfg);
      const ok = await connect();
      setMsg(
        ok
          ? "Connecté ✓"
          : "Enregistré, mais l'agent n'est pas joignable — vérifie clé/modèle ou que `aonyx` (build --features api) est sur le PATH.",
      );
    } catch (e) {
      setMsg("Échec : " + e);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={SettingsIcon} title={t("nav.settings")} />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-2xl space-y-8">
          {/* Provider */}
          <section className="space-y-4">
            <h2 className="font-cond uppercase tracking-wide text-sm text-aonyx-500">{t("settings.providerSection")}</h2>
            <Field label={t("settings.provider")}>
              <select value={provider} onChange={(e) => onProviderChange(e.target.value)} className={inputCls}>
                {PROVIDERS.map((p) => (
                  <option key={p.id} value={p.id}>{p.label}</option>
                ))}
              </select>
            </Field>
            <Field label={t("settings.model")}>
              <div className="flex gap-2">
                <select value={model} onChange={(e) => setModel(e.target.value)} className={`${inputCls} flex-1`}>
                  {models.map((m) => (
                    <option key={m} value={m}>{m}</option>
                  ))}
                  <option value={CUSTOM}>Custom… (saisir l'id)</option>
                </select>
                <button
                  onClick={() => loadModels(provider, selectedModel() || null, apiKey, base)}
                  disabled={loadingModels}
                  title="Recharger les modèles"
                  className="flex items-center justify-center w-10 rounded-lg border border-aonyx-300 dark:border-aonyx-700 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 disabled:opacity-50"
                >
                  <RefreshCw className={`w-4 h-4 ${loadingModels ? "animate-spin" : ""}`} />
                </button>
              </div>
              {modelNote && <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">{modelNote}</p>}
            </Field>
            {model === CUSTOM && (
              <Field label="Id du modèle (custom)">
                <input value={customModel} onChange={(e) => setCustomModel(e.target.value)} className={inputCls} placeholder="exact model id" spellCheck={false} />
              </Field>
            )}
            {NEEDS_KEY.has(provider) && (
              <Field label="Clé API">
                <input
                  type="password"
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  onBlur={() => loadModels(provider, selectedModel() || null, apiKey, base)}
                  className={inputCls}
                  placeholder="sk-…"
                  spellCheck={false}
                />
              </Field>
            )}
            {HAS_BASE.has(provider) && (
              <Field label="Base URL">
                <input value={base} onChange={(e) => setBase(e.target.value)} className={inputCls} placeholder={DEFAULT_BASE[provider] || "https://…"} spellCheck={false} />
              </Field>
            )}
            {provider === "claude-code" && (
              <Field label="Binaire claude">
                <input value={binary} onChange={(e) => setBinary(e.target.value)} className={inputCls} placeholder="claude" spellCheck={false} />
              </Field>
            )}
          </section>

          {/* Connection */}
          <section className="space-y-4">
            <h2 className="font-cond uppercase tracking-wide text-sm text-aonyx-500">{t("settings.connectionSection")}</h2>
            <label className="flex items-center gap-2.5 text-sm text-aonyx-700 dark:text-aonyx-200">
              <input type="checkbox" checked={local} onChange={(e) => setLocal(e.target.checked)} className="accent-primary-600" />
              Agent local embarqué (lance <code className="font-mono text-xs text-aonyx-500">aonyx serve api</code>)
            </label>
            <Field label="URL de l'API">
              <input value={apiUrl} onChange={(e) => setApiUrl(e.target.value)} disabled={local} className={inputCls} placeholder="http://127.0.0.1:8788" spellCheck={false} />
            </Field>
            <Field label="Token (optionnel)">
              <input type="password" value={token} onChange={(e) => setToken(e.target.value)} disabled={local} className={inputCls} placeholder="bearer" spellCheck={false} />
            </Field>
          </section>

          <div className="flex items-center gap-3 pb-4">
            <button onClick={save} disabled={saving} className="px-4 py-2 rounded-lg bg-primary-600 hover:bg-primary-700 text-white font-medium disabled:opacity-50">
              {t("settings.save")}
            </button>
            {msg && <span className="text-sm text-aonyx-500">{msg}</span>}
          </div>
        </div>
      </div>
    </div>
  );
}
