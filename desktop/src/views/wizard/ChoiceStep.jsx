import { useState } from "react";
import { useI18n } from "../../context/LanguageContext";

// Generic two-card chooser, reused for the RAG-backend and embeddings screens.
// `field` is the draft key the selection writes to.
export default function ChoiceStep({ titleKey, descKey, options, value, field, onNext, onBack }) {
  const { t } = useI18n();
  const [sel, setSel] = useState(value || options[0]?.id);

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-medium text-aonyx-900 dark:text-aonyx-50">{t(titleKey)}</h2>
        <p className="text-sm text-aonyx-500 mt-0.5">{t(descKey)}</p>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        {options.map((o) => {
          const Icon = o.icon;
          const active = sel === o.id;
          return (
            <button
              key={o.id}
              onClick={() => setSel(o.id)}
              className={`text-left rounded-xl border p-4 transition-colors ${
                active
                  ? "border-primary-600 ring-1 ring-primary-600 bg-primary-600/5"
                  : "border-aonyx-200 dark:border-aonyx-800 hover:border-aonyx-300 dark:hover:border-aonyx-700"
              }`}
            >
              <div className="flex items-center gap-2 mb-1.5">
                <Icon className={`w-5 h-5 ${active ? "text-primary-600" : "text-aonyx-500"}`} />
                <span className="font-medium text-aonyx-900 dark:text-aonyx-50">{t(o.labelKey)}</span>
                {o.recommended && (
                  <span className="ml-auto text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-primary-600/15 text-primary-600">
                    {t("wizard.badge.recommended")}
                  </span>
                )}
              </div>
              <p className="text-xs text-aonyx-500 leading-relaxed">{t(o.descKey)}</p>
            </button>
          );
        })}
      </div>

      <div className="flex justify-between pt-2">
        <button onClick={onBack} className="px-4 py-2 rounded-lg border border-aonyx-300 dark:border-aonyx-700 hover:bg-aonyx-100 dark:hover:bg-aonyx-900">
          {t("wizard.back")}
        </button>
        <button onClick={() => onNext({ [field]: sel })} className="px-4 py-2 rounded-lg bg-primary-600 hover:bg-primary-700 text-white font-medium">
          {t("wizard.continue")}
        </button>
      </div>
    </div>
  );
}
