import { useState } from "react";
import TitleBar from "../../layout/TitleBar";
import { useI18n } from "../../context/LanguageContext";
import ProviderStep from "./ProviderStep";
import ChoiceStep from "./ChoiceStep";
import BootstrapStep from "./BootstrapStep";
import logo from "../../assets/logo.png";
import { Database, Cloud, Cpu, Server } from "lucide-react";

// First-run onboarding (ADR-016). Four screens: provider → RAG → embeddings →
// auto-bootstrap. The agent is single-binary Rust, so this stays light — no
// Python/venv/build steps (that's Hermes). `onDone` swaps to the app.
const STEPS = ["provider", "rag", "embeddings", "setup"];

export default function Wizard({ onDone }) {
  const { t } = useI18n();
  const [i, setI] = useState(0);
  const [draft, setDraft] = useState({
    provider: "anthropic",
    model: "",
    apiKey: "",
    base: "",
    binary: "",
    rag_backend: "local",
    rag_embeddings: "local",
  });

  const step = STEPS[i];
  const merge = (patch) => setDraft((d) => ({ ...d, ...patch }));
  const next = (patch) => { if (patch) merge(patch); setI((n) => Math.min(STEPS.length - 1, n + 1)); };
  const back = () => setI((n) => Math.max(0, n - 1));

  return (
    <div className="flex flex-col h-screen overflow-hidden bg-aonyx-50 dark:bg-aonyx-900">
      <TitleBar />
      <div className="flex-1 min-h-0 overflow-y-auto flex items-center justify-center p-6">
        <div className="w-full max-w-xl">
          <div className="flex items-center gap-3 mb-5">
            <img src={logo} alt="" className="w-10 h-10 rounded-xl" />
            <div>
              <h1 className="text-lg font-medium text-aonyx-900 dark:text-aonyx-50 leading-tight">{t("wizard.title")}</h1>
              <p className="text-sm text-aonyx-500">{t("wizard.subtitle")}</p>
            </div>
          </div>

          <ol className="flex items-center gap-2 mb-5">
            {STEPS.map((s, idx) => (
              <li key={s} className="flex items-center gap-2 flex-1">
                <span
                  className={`flex items-center justify-center w-6 h-6 rounded-full text-xs font-medium shrink-0 ${
                    idx < i
                      ? "bg-primary-600 text-white"
                      : idx === i
                        ? "bg-primary-600/15 text-primary-600 ring-1 ring-primary-600"
                        : "bg-aonyx-200/70 dark:bg-aonyx-800 text-aonyx-500"
                  }`}
                >
                  {idx + 1}
                </span>
                <span className={`text-xs truncate ${idx === i ? "text-aonyx-900 dark:text-aonyx-100" : "text-aonyx-500"}`}>
                  {t(`wizard.step.${s}`)}
                </span>
              </li>
            ))}
          </ol>

          <div className="rounded-2xl border border-aonyx-200 dark:border-aonyx-800 bg-white dark:bg-aonyx-950 p-6">
            {step === "provider" && <ProviderStep draft={draft} onNext={next} />}
            {step === "rag" && (
              <ChoiceStep
                titleKey="wizard.rag.title"
                descKey="wizard.rag.desc"
                value={draft.rag_backend}
                field="rag_backend"
                options={[
                  { id: "local", icon: Database, labelKey: "wizard.rag.local.label", descKey: "wizard.rag.local.desc", recommended: true },
                  { id: "external", icon: Cloud, labelKey: "wizard.rag.external.label", descKey: "wizard.rag.external.desc" },
                ]}
                onNext={next}
                onBack={back}
              />
            )}
            {step === "embeddings" && (
              <ChoiceStep
                titleKey="wizard.embeddings.title"
                descKey="wizard.embeddings.desc"
                value={draft.rag_embeddings}
                field="rag_embeddings"
                options={[
                  { id: "local", icon: Cpu, labelKey: "wizard.embeddings.local.label", descKey: "wizard.embeddings.local.desc", recommended: true },
                  { id: "provider", icon: Server, labelKey: "wizard.embeddings.provider.label", descKey: "wizard.embeddings.provider.desc" },
                ]}
                onNext={next}
                onBack={back}
              />
            )}
            {step === "setup" && <BootstrapStep draft={draft} onDone={onDone} />}
          </div>
        </div>
      </div>
    </div>
  );
}
