import { useState } from "react";
import {
  SlidersHorizontal,
  MessagesSquare,
  Database,
  Download,
  FolderOpen,
  LayoutDashboard,
  BarChart3,
  Users as UsersIcon,
  Shield,
  Info,
} from "lucide-react";
import { useI18n } from "../context/LanguageContext";
import ProviderConfig from "./Settings";
import Dashboard from "./Dashboard";
import Projets from "./Projets";
import Stats from "./Stats";
import KnowledgeGraph from "./KnowledgeGraph";
import Mcp from "./Mcp";
import Messaging from "./Messaging";
import About from "./About";
import { Users, Permissions } from "./index";

// Settings hub (Hermes-style IA): the main sidebar shows only conversations;
// everything else lives here, grouped (preferences · integrations · admin · system).
const GROUPS = [
  {
    key: "settings.group.prefs",
    items: [
      { id: "provider", key: "settings.section.provider", icon: SlidersHorizontal, El: ProviderConfig },
      { id: "memory", key: "nav.kg", icon: Database, El: KnowledgeGraph },
      { id: "safety", key: "nav.permissions", icon: Shield, El: Permissions },
    ],
  },
  {
    key: "settings.group.integrations",
    items: [
      { id: "gateway", key: "nav.messaging", icon: MessagesSquare, El: Messaging },
      { id: "mcp", key: "nav.mcp", icon: Download, El: Mcp },
      { id: "workspace", key: "nav.projects", icon: FolderOpen, El: Projets },
    ],
  },
  {
    key: "settings.group.admin",
    items: [
      { id: "dashboard", key: "nav.dashboard", icon: LayoutDashboard, El: Dashboard },
      { id: "stats", key: "nav.stats", icon: BarChart3, El: Stats },
      { id: "users", key: "nav.users", icon: UsersIcon, El: Users },
    ],
  },
  {
    key: "settings.group.system",
    items: [{ id: "about", key: "nav.about", icon: Info, El: About }],
  },
];

const ALL = GROUPS.flatMap((g) => g.items);

export default function SettingsHub() {
  const { t } = useI18n();
  const [active, setActive] = useState("provider");
  const section = ALL.find((s) => s.id === active) || ALL[0];
  const El = section.El;
  return (
    <div className="flex h-full">
      <aside className="w-52 flex-shrink-0 flex flex-col border-r border-aonyx-200 dark:border-aonyx-800 overflow-y-auto p-2">
        <span className="text-[11px] font-cond uppercase tracking-wider text-aonyx-500 px-2 pt-2 pb-1">{t("nav.settings")}</span>
        {GROUPS.map((g, gi) => (
          <div key={g.key} className={gi > 0 ? "mt-3 pt-2 border-t border-aonyx-200/70 dark:border-aonyx-800/70" : ""}>
            <span className="block text-[10px] uppercase tracking-wider text-aonyx-400 px-3 pb-1">{t(g.key)}</span>
            {g.items.map((s) => {
              const Icon = s.icon;
              const isActive = active === s.id;
              return (
                <button
                  key={s.id}
                  onClick={() => setActive(s.id)}
                  className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-colors ${
                    isActive
                      ? "bg-primary-100 dark:bg-primary-950/40 text-primary-800 dark:text-primary-200"
                      : "text-aonyx-600 dark:text-aonyx-400 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 hover:text-aonyx-900 dark:hover:text-aonyx-100"
                  }`}
                >
                  <Icon className="w-[18px] h-[18px] flex-shrink-0" strokeWidth={1.75} />
                  <span className="font-medium truncate">{t(s.key)}</span>
                </button>
              );
            })}
          </div>
        ))}
      </aside>
      <div className="flex-1 min-w-0 overflow-hidden">
        <El />
      </div>
    </div>
  );
}
