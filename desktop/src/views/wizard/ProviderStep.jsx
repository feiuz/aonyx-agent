import { useCallback, useEffect, useState } from "react";
import { RefreshCw, LogIn } from "lucide-react";
import { useI18n } from "../../context/LanguageContext";
import { listModels, claudeLogin } from "../../services/configService";

// Provider screen of the wizard. Mirrors Settings' provider logic (same PROVIDERS
// / list_models contract) but scoped to onboarding — the choice flows into the
// draft and is persisted once, at the bootstrap step.
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

export default function ProviderStep({ draft, onNext }) {
  const { t } = useI18n();
  const [provider, setProvider] = useState(draft.provider || "anthropic");
  const [model, setModel] = useState(draft.model || "");
  const [customModel, setCustomModel] = useState("");
  const [models, setModels] = useState([]);
  const [note, setNote] = useState("");
  const [loading, setLoading] = useState(false);
  const [authIssue, setAuthIssue] = useState(false); // claude-code token expired/absent
  const [apiKey, setApiKey] = useState(draft.apiKey || "");
  const [base, setBase] = useState(draft.base || "");
  const [binary, setBinary] = useState(draft.binary || "");

  const load = useCallback(
    async (prov, current, keyVal, baseVal) => {
      setLoading(true);
      setNote(t("settings.model.loading"));
      try {
        const list = (await listModels(prov, baseVal || DEFAULT_BASE[prov] || "", keyVal || "")) || [];
        const finalList = current && !list.includes(current) ? [current, ...list] : list;
        setModels(finalList);
        if (!current && finalList.length) setModel(finalList[0]);
        setAuthIssue(false);
        setNote(list.length ? `${list.length} ${t("settings.model.available")}` : t("settings.model.none"));
      } catch (e) {
        const m = String(e);
        setModels(current ? [current] : []);
        const cc = m.includes("CLAUDE_CODE_EXPIRED") || m.includes("CLAUDE_CODE_ABSENT");
        setAuthIssue(cc);
        setNote(
          m.includes("API_KEY_REQUIRED")
            ? t("settings.model.keyRequired")
            : m.includes("CLAUDE_CODE_EXPIRED")
              ? t("settings.model.ccExpired")
              : m.includes("CLAUDE_CODE_ABSENT")
                ? t("settings.model.ccAbsent")
                : t("settings.model.fetchFail") + m,
        );
      } finally {
        setLoading(false);
      }
    },
    [t],
  );

  useEffect(() => {
    load(provider, model || null, apiKey, base);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onProviderChange = (p) => {
    setProvider(p);
    setModel("");
    setAuthIssue(false);
    load(p, null, apiKey, base);
  };

  const selected = () => (model === CUSTOM ? customModel.trim() : model);
  const canContinue = !!selected() && (!NEEDS_KEY.has(provider) || apiKey.trim().length > 0);

  // Re-login: relaunch Claude Code (it refreshes its own token), then retry the
  // model fetch once it has had a moment to persist fresh credentials.
  const relogin = async () => {
    try {
      await claudeLogin(binary);
      setNote(t("settings.model.ccRelaunched"));
      setTimeout(() => load(provider, selected() || null, apiKey, base), 5000);
    } catch (e) {
      setNote(String(e));
    }
  };

  const cont = () =>
    onNext({ provider, model: selected(), apiKey, base: base.trim(), binary: binary.trim() });

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-medium text-aonyx-900 dark:text-aonyx-50">{t("wizard.provider.title")}</h2>
        <p className="text-sm text-aonyx-500 mt-0.5">{t("wizard.provider.desc")}</p>
      </div>

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
            <option value={CUSTOM}>{t("settings.customOption")}</option>
          </select>
          <button
            onClick={() => load(provider, selected() || null, apiKey, base)}
            disabled={loading}
            aria-label="reload models"
            className="flex items-center justify-center w-10 rounded-lg border border-aonyx-300 dark:border-aonyx-700 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 disabled:opacity-50"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
          </button>
        </div>
        {note && <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">{note}</p>}
        {authIssue && (
          <button
            onClick={relogin}
            className="mt-2 inline-flex items-center gap-1.5 text-xs px-2.5 py-1.5 rounded-lg border border-amber-500/50 text-amber-700 dark:text-amber-300 hover:bg-amber-500/10"
          >
            <LogIn className="w-3.5 h-3.5" />
            {t("settings.model.ccRelogin")}
          </button>
        )}
      </Field>

      {model === CUSTOM && (
        <Field label={t("settings.customModel")}>
          <input value={customModel} onChange={(e) => setCustomModel(e.target.value)} className={inputCls} placeholder="exact model id" spellCheck={false} />
        </Field>
      )}
      {NEEDS_KEY.has(provider) && (
        <Field label={t("settings.apiKey")}>
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            onBlur={() => load(provider, selected() || null, apiKey, base)}
            className={inputCls}
            placeholder="sk-…"
            spellCheck={false}
          />
        </Field>
      )}
      {HAS_BASE.has(provider) && (
        <Field label={t("settings.baseUrl")}>
          <input value={base} onChange={(e) => setBase(e.target.value)} className={inputCls} placeholder={DEFAULT_BASE[provider] || "https://…"} spellCheck={false} />
        </Field>
      )}
      {provider === "claude-code" && (
        <Field label={t("settings.binary")}>
          <input value={binary} onChange={(e) => setBinary(e.target.value)} className={inputCls} placeholder="claude" spellCheck={false} />
        </Field>
      )}

      <div className="flex justify-end pt-2">
        <button
          onClick={cont}
          disabled={!canContinue}
          className="px-4 py-2 rounded-lg bg-primary-600 hover:bg-primary-700 text-white font-medium disabled:opacity-50"
        >
          {t("wizard.continue")}
        </button>
      </div>
    </div>
  );
}
