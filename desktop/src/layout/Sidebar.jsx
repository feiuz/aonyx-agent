import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  PanelLeft,
  Sun,
  Moon,
  Plus,
  Search,
  Settings as SettingsIcon,
  User,
  Languages,
  Cpu,
  ArrowUpCircle,
  MessageSquare,
  Pin,
} from "lucide-react";
import { useTheme } from "../context/ThemeContext";
import { useAuth } from "../context/AuthContext";
import { useI18n } from "../context/LanguageContext";
import { useAgent } from "../context/AgentContext";
import { isTauri, safeInvoke } from "../config/bridge";
import { readProviderConfig } from "../services/configService";

const PROVIDER_LABEL = {
  anthropic: "Anthropic",
  openai: "OpenAI",
  openrouter: "OpenRouter",
  ollama: "Ollama",
  "lm-studio": "LM Studio",
  "claude-code": "Claude Code",
};

const loadPinned = () => {
  try {
    return JSON.parse(localStorage.getItem("aonyx.pinned") || "[]");
  } catch {
    return [];
  }
};

// Hermes-style main sidebar: top controls, new conversation (Ctrl+N), search,
// PINNED + SESSIONS sections, account at the bottom. Everything else is in Settings.
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
  const [query, setQuery] = useState("");
  const [pinned, setPinned] = useState(loadPinned);

  useEffect(() => {
    localStorage.setItem("aonyx.sidebarCollapsed", collapsed ? "1" : "0");
  }, [collapsed]);
  useEffect(() => {
    localStorage.setItem("aonyx.pinned", JSON.stringify(pinned));
  }, [pinned]);

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

  const newConversation = async () => {
    try {
      await createSession();
    } catch {
      /* ignore */
    }
    navigate("/");
  };

  useEffect(() => {
    const onKey = (e) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "n" || e.key === "N")) {
        e.preventDefault();
        newConversation();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onItem = (e, id) => {
    if (e.shiftKey) {
      setPinned((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
    } else {
      setSessionId(id);
      navigate("/");
    }
  };

  const filtered = sessions.filter(
    (s) => !query || (s.title || "").toLowerCase().includes(query.toLowerCase()),
  );
  const pinnedList = filtered.filter((s) => pinned.includes(s.id));
  const rest = filtered.filter((s) => !pinned.includes(s.id));

  const iconBtn =
    "flex items-center justify-center w-8 h-8 rounded-md text-aonyx-500 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 hover:text-aonyx-800 dark:hover:text-aonyx-200 transition-colors";

  const item = (s) => {
    const active = s.id === sessionId;
    const isPinned = pinned.includes(s.id);
    return (
      <button
        key={s.id}
        onClick={(e) => onItem(e, s.id)}
        title={s.title || t("chat.untitled")}
        className={`group w-full flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-left text-sm transition-colors ${
          active
            ? "bg-aonyx-200/70 dark:bg-aonyx-800/70 text-aonyx-900 dark:text-aonyx-100"
            : "text-aonyx-600 dark:text-aonyx-400 hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/50"
        }`}
      >
        {isPinned ? (
          <Pin className="w-3 h-3 flex-shrink-0 text-primary-500" strokeWidth={2} />
        ) : (
          <span className="w-1.5 h-1.5 rounded-full bg-aonyx-400 flex-shrink-0 ml-0.5 mr-0.5" />
        )}
        <span className="truncate">{s.title || t("chat.untitled")}</span>
      </button>
    );
  };

  const sectionHeader = (label, count) => (
    <div className="flex items-center gap-1.5 px-2.5 pt-3 pb-1">
      <span className="w-1.5 h-1.5 rotate-45 bg-primary-500/80" />
      <span className="text-[11px] font-cond uppercase tracking-wider text-primary-700 dark:text-primary-400">{label}</span>
      {count != null && <span className="text-[11px] text-aonyx-400">{count}</span>}
    </div>
  );

  return (
    <aside
      className={`${collapsed ? "w-14" : "w-64"} flex-shrink-0 flex flex-col bg-aonyx-100 dark:bg-aonyx-950 border-r border-aonyx-200 dark:border-aonyx-800 transition-[width] duration-200`}
    >
      <div className="flex items-center justify-between h-11 px-2 flex-shrink-0">
        <button onClick={() => setCollapsed((c) => !c)} className={iconBtn} title={collapsed ? t("sidebar.expand") : t("sidebar.collapse")}>
          <PanelLeft className="w-[18px] h-[18px]" strokeWidth={1.75} />
        </button>
        {!collapsed && (
          <button onClick={toggle} className={iconBtn} title={theme === "dark" ? t("theme.toLight") : t("theme.toDark")}>
            {theme === "dark" ? <Sun className="w-[18px] h-[18px]" /> : <Moon className="w-[18px] h-[18px]" />}
          </button>
        )}
      </div>

      <div className="px-2">
        <button
          onClick={newConversation}
          title={collapsed ? t("sidebar.newChat") : ""}
          className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2.5"} px-2.5 py-2 rounded-lg text-sm font-medium hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 transition-colors`}
        >
          <Plus className="w-[18px] h-[18px] flex-shrink-0" strokeWidth={1.75} />
          {!collapsed && (
            <>
              <span>{t("sidebar.newChat")}</span>
              <span className="ml-auto flex items-center gap-1 text-[10px] text-aonyx-400">
                <kbd className="px-1 py-0.5 rounded border border-aonyx-300 dark:border-aonyx-700 font-sans">Ctrl</kbd>
                <kbd className="px-1 py-0.5 rounded border border-aonyx-300 dark:border-aonyx-700 font-sans">N</kbd>
              </span>
            </>
          )}
        </button>
      </div>

      {!collapsed && (
        <div className="px-2 pt-1">
          <div className="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-aonyx-200/40 dark:bg-aonyx-900/40">
            <Search className="w-4 h-4 text-aonyx-400 flex-shrink-0" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("sidebar.search")}
              className="flex-1 min-w-0 bg-transparent text-sm focus:outline-none placeholder:text-aonyx-400"
            />
          </div>
        </div>
      )}

      <nav className="flex-1 overflow-y-auto px-2 pb-2 min-h-0">
        {!collapsed ? (
          <>
            {sectionHeader(t("sidebar.pinned"))}
            {pinnedList.length === 0 ? (
              <p className="text-xs text-aonyx-400 px-2.5 py-1">{t("sidebar.pinHint")}</p>
            ) : (
              <div className="space-y-0.5">{pinnedList.map(item)}</div>
            )}
            {sectionHeader(t("chat.conversations"), rest.length)}
            {rest.length === 0 ? (
              <p className="text-xs text-aonyx-400 px-2.5 py-1">{t("chat.none")}</p>
            ) : (
              <div className="space-y-0.5">{rest.map(item)}</div>
            )}
          </>
        ) : (
          <div className="space-y-1 pt-1">
            {filtered.slice(0, 14).map((s) => (
              <button
                key={s.id}
                onClick={() => { setSessionId(s.id); navigate("/"); }}
                title={s.title || t("chat.untitled")}
                className={`w-full flex justify-center py-1.5 rounded-lg ${s.id === sessionId ? "bg-aonyx-200/70 dark:bg-aonyx-800/70 text-aonyx-900 dark:text-aonyx-100" : "text-aonyx-500 hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/50"}`}
              >
                <MessageSquare className="w-4 h-4" strokeWidth={1.75} />
              </button>
            ))}
          </div>
        )}
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

        {llm && (
          <button
            onClick={() => navigate("/settings")}
            title={`${PROVIDER_LABEL[llm.provider] || llm.provider} · ${llm.model}`}
            className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2.5"} px-2.5 py-1.5 rounded-lg hover:bg-aonyx-200/50 dark:hover:bg-aonyx-900/60 transition-colors`}
          >
            <Cpu className="w-4 h-4 text-aonyx-500 flex-shrink-0" strokeWidth={1.75} />
            {!collapsed && (
              <span className="flex flex-col items-start min-w-0 leading-tight">
                <span className="text-xs font-medium text-aonyx-700 dark:text-aonyx-300 truncate max-w-[180px]">{PROVIDER_LABEL[llm.provider] || llm.provider}</span>
                <span className="text-[10px] text-aonyx-500 truncate max-w-[180px]">{llm.model}</span>
              </span>
            )}
          </button>
        )}

        <div className={`flex items-center ${collapsed ? "flex-col gap-1" : "gap-1"}`}>
          <button onClick={() => navigate("/settings")} title={t("nav.settings")} className={iconBtn}>
            <SettingsIcon className="w-4 h-4" />
          </button>
          <button onClick={toggleLang} title={lang === "fr" ? "English" : "Français"} className={`${iconBtn} gap-1 ${collapsed ? "" : "px-2 w-auto"} text-[11px] font-mono uppercase`}>
            <Languages className="w-4 h-4" />
            {!collapsed && (lang === "fr" ? "EN" : "FR")}
          </button>
          {collapsed && (
            <button onClick={toggle} className={iconBtn} title={theme === "dark" ? t("theme.toLight") : t("theme.toDark")}>
              {theme === "dark" ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
            </button>
          )}
        </div>

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
              <span className="text-sm font-medium text-aonyx-900 dark:text-aonyx-100 truncate">{isAuthenticated ? user?.email : t("auth.signin")}</span>
              <span className="text-[11px] text-aonyx-500 truncate">{isAuthenticated ? user?.tier || "FREE" : "aonyx-account"}</span>
            </span>
          )}
        </button>
      </div>
    </aside>
  );
}
