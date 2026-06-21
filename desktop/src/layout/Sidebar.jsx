import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Plus,
  MessageSquare,
  Settings as SettingsIcon,
  ChevronLeft,
  ChevronRight,
  Sun,
  Moon,
  ArrowUpCircle,
  User,
  Languages,
  Cpu,
} from "lucide-react";
import { useTheme } from "../context/ThemeContext";
import { useAuth } from "../context/AuthContext";
import { useI18n } from "../context/LanguageContext";
import { useAgent } from "../context/AgentContext";
import { isTauri, safeInvoke } from "../config/bridge";
import { readProviderConfig } from "../services/configService";
import logo from "../assets/logo.png";

const PROVIDER_LABEL = {
  anthropic: "Anthropic",
  openai: "OpenAI",
  openrouter: "OpenRouter",
  ollama: "Ollama",
  "lm-studio": "LM Studio",
  "claude-code": "Claude Code",
};

// Hermes-style main sidebar: conversations only (everything else is in Settings).
export default function Sidebar() {
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem("aonyx.sidebarCollapsed") === "1",
  );
  const { theme, toggle } = useTheme();
  const { isAuthenticated, user, signIn, logout } = useAuth();
  const { t, lang, toggle: toggleLang } = useI18n();
  const { sessions, sessionId, setSessionId, createSession } = useAgent();
  const navigate = useNavigate();
  const [update, setUpdate] = useState(null);
  const [llm, setLlm] = useState(null);

  useEffect(() => {
    localStorage.setItem("aonyx.sidebarCollapsed", collapsed ? "1" : "0");
  }, [collapsed]);

  useEffect(() => {
    if (!isTauri()) return;
    const tm = setTimeout(async () => {
      const u = await safeInvoke("check_for_update");
      if (u?.version) setUpdate(u);
    }, 3000);
    return () => clearTimeout(tm);
  }, []);

  useEffect(() => {
    const read = async () => {
      const c = await readProviderConfig();
      setLlm(c?.model ? { provider: c.provider, model: c.model } : null);
    };
    read();
    window.addEventListener("aonyx:provider-changed", read);
    return () => window.removeEventListener("aonyx:provider-changed", read);
  }, []);

  const openSession = (id) => {
    setSessionId(id);
    navigate("/");
  };
  const newConversation = async () => {
    try {
      await createSession();
    } catch {
      /* ignore */
    }
    navigate("/");
  };

  const iconBtn =
    "flex items-center justify-center w-8 h-8 rounded-md text-aonyx-500 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 hover:text-aonyx-800 dark:hover:text-aonyx-200 transition-colors";

  return (
    <aside
      className={`${collapsed ? "w-16" : "w-64"} flex-shrink-0 flex flex-col bg-aonyx-100 dark:bg-aonyx-950 border-r border-aonyx-200 dark:border-aonyx-800 transition-[width] duration-200`}
    >
      <div className="flex items-center gap-2.5 h-14 px-3 flex-shrink-0 border-b border-aonyx-200 dark:border-aonyx-800">
        <img src={logo} alt="" className="w-7 h-7 rounded-lg flex-shrink-0" />
        {!collapsed && <span className="font-cond uppercase tracking-wide text-aonyx-900 dark:text-aonyx-100">Aonyx</span>}
      </div>

      <div className="p-2">
        <button
          onClick={newConversation}
          title={collapsed ? t("chat.new") : ""}
          className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2"} px-3 py-2 rounded-lg text-sm font-medium border border-aonyx-300 dark:border-aonyx-700 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 transition-colors`}
        >
          <Plus className="w-4 h-4 flex-shrink-0" />
          {!collapsed && t("chat.new")}
        </button>
      </div>

      <nav className="flex-1 overflow-y-auto px-2 space-y-0.5 min-h-0">
        {!collapsed && (
          <span className="block text-[11px] font-cond uppercase tracking-wider text-aonyx-500 px-2 py-1.5">
            {t("chat.conversations")}
          </span>
        )}
        {sessions.length === 0 && !collapsed && (
          <p className="text-xs text-aonyx-500 px-2 py-1">{t("chat.none")}</p>
        )}
        {sessions.map((s) => {
          const active = s.id === sessionId;
          return (
            <button
              key={s.id}
              onClick={() => openSession(s.id)}
              title={collapsed ? s.title || t("chat.untitled") : ""}
              className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2.5"} px-2.5 py-2 rounded-lg text-left transition-colors ${
                active
                  ? "bg-aonyx-200/70 dark:bg-aonyx-800/70 text-aonyx-900 dark:text-aonyx-100"
                  : "text-aonyx-600 dark:text-aonyx-400 hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/50"
              }`}
            >
              <MessageSquare className="w-4 h-4 flex-shrink-0" strokeWidth={1.75} />
              {!collapsed && (
                <span className="flex flex-col min-w-0 leading-tight">
                  <span className="truncate text-sm">{s.title || t("chat.untitled")}</span>
                  <span className="text-[11px] font-mono text-aonyx-500">
                    {s.turns} {s.turns === 1 ? t("chat.turn") : t("chat.turns")}
                  </span>
                </span>
              )}
            </button>
          );
        })}
      </nav>

      <div className="p-2 border-t border-aonyx-200 dark:border-aonyx-800 space-y-1.5">
        {update && (
          <button
            title={`${t("update.label")} ${update.version}`}
            className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2"} px-3 py-1.5 rounded-md text-emerald-700 dark:text-emerald-400 hover:bg-emerald-50 dark:hover:bg-emerald-950/30 transition-colors`}
          >
            <ArrowUpCircle className="w-4 h-4 flex-shrink-0" strokeWidth={1.75} />
            {!collapsed && <span className="text-xs font-medium truncate">{t("update.label")} {update.version}</span>}
          </button>
        )}

        <div className={`flex items-center ${collapsed ? "flex-col gap-1" : "gap-1"}`}>
          <button onClick={toggle} title={theme === "dark" ? t("theme.toLight") : t("theme.toDark")} className={iconBtn}>
            {theme === "dark" ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
          </button>
          <button onClick={toggleLang} title={lang === "fr" ? "English" : "Français"} className={`${iconBtn} gap-1 px-2 w-auto text-[11px] font-mono uppercase`}>
            <Languages className="w-4 h-4" />
            {!collapsed && (lang === "fr" ? "EN" : "FR")}
          </button>
          <button onClick={() => navigate("/settings")} title={t("nav.settings")} className={iconBtn}>
            <SettingsIcon className="w-4 h-4" />
          </button>
          {!collapsed && <span className="flex-1" />}
          <button onClick={() => setCollapsed((c) => !c)} title={collapsed ? t("sidebar.expand") : t("sidebar.collapse")} className={iconBtn}>
            {collapsed ? <ChevronRight className="w-4 h-4" /> : <ChevronLeft className="w-4 h-4" />}
          </button>
        </div>

        {llm && (
          <button
            onClick={() => navigate("/settings")}
            title={`${PROVIDER_LABEL[llm.provider] || llm.provider} · ${llm.model}`}
            className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2.5"} px-2.5 py-1.5 rounded-lg hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/60 transition-colors`}
          >
            <Cpu className="w-4 h-4 text-aonyx-500 flex-shrink-0" strokeWidth={1.75} />
            {!collapsed && (
              <span className="flex flex-col items-start min-w-0 leading-tight">
                <span className="text-xs font-medium text-aonyx-700 dark:text-aonyx-300 truncate max-w-[180px]">
                  {PROVIDER_LABEL[llm.provider] || llm.provider}
                </span>
                <span className="text-[10px] text-aonyx-500 truncate max-w-[180px]">{llm.model}</span>
              </span>
            )}
          </button>
        )}

        <button
          onClick={() => (isAuthenticated ? logout() : signIn())}
          title={isAuthenticated ? t("auth.signout") : t("auth.signin")}
          className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2.5"} px-2.5 py-2 rounded-lg bg-aonyx-200/50 dark:bg-aonyx-900/60 hover:bg-aonyx-200 dark:hover:bg-aonyx-900 transition-colors`}
        >
          <span className="flex items-center justify-center w-7 h-7 rounded-full bg-aonyx-300 dark:bg-aonyx-800 text-aonyx-700 dark:text-aonyx-200 flex-shrink-0">
            <User className="w-4 h-4" strokeWidth={1.75} />
          </span>
          {!collapsed && (
            <span className="flex flex-col items-start min-w-0 leading-tight">
              <span className="text-sm font-medium text-aonyx-900 dark:text-aonyx-100 truncate">
                {isAuthenticated ? user?.email : t("auth.signin")}
              </span>
              <span className="text-[11px] text-aonyx-500 truncate">
                {isAuthenticated ? user?.tier || "FREE" : "aonyx-account"}
              </span>
            </span>
          )}
        </button>
      </div>
    </aside>
  );
}
