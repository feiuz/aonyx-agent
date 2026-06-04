import { createContext, useContext, useEffect, useState } from "react";
import { DICTS, detectLang } from "../i18n";

const LanguageContext = createContext(null);

export function LanguageProvider({ children }) {
  const [lang, setLang] = useState(detectLang);

  useEffect(() => {
    localStorage.setItem("aonyx.lang", lang);
    document.documentElement.lang = lang;
  }, [lang]);

  // t(key) → translated string; falls back to FR, then the key itself.
  const t = (key, fallback) => DICTS[lang]?.[key] ?? DICTS.fr[key] ?? fallback ?? key;
  const toggle = () => setLang((l) => (l === "fr" ? "en" : "fr"));

  return (
    <LanguageContext.Provider value={{ lang, setLang, toggle, t }}>
      {children}
    </LanguageContext.Provider>
  );
}

export const useI18n = () => useContext(LanguageContext);
