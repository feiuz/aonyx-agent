import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  MessageSquare,
  FolderOpen,
  BarChart3,
  Activity,
  Database,
  Users as UsersIcon,
  Shield,
  Download,
  Settings as SettingsIcon,
  ChevronLeft,
  ChevronRight,
  Sun,
  Moon,
  ArrowUpCircle,
  User,
} from "lucide-react";
import { useTheme } from "../context/ThemeContext";
import { useAuth } from "../context/AuthContext";
import { isTauri, safeInvoke } from "../config/bridge";

const NAV = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/chat", label: "Chat", icon: MessageSquare },
  { to: "/projects", label: "Projets", icon: FolderOpen },
  { to: "/stats", label: "Statistiques", icon: BarChart3 },
  { to: "/memory-health", label: "Memory Health", icon: Activity },
  { to: "/kg", label: "Knowledge Graph", icon: Database },
  { to: "/users", label: "Utilisateurs", icon: UsersIcon },
  { to: "/permissions", label: "Permissions", icon: Shield },
  { to: "/mcp", label: "MCP", icon: Download },
  { to: "/settings", label: "Paramètres", icon: SettingsIcon },
];

export default function Sidebar() {
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem("aonyx.sidebarCollapsed") === "1",
  );
  const { theme, toggle } = useTheme();
  const { isAuthenticated, user, signIn, logout } = useAuth();
  const [update, setUpdate] = useState(null);

  useEffect(() => {
    localStorage.setItem("aonyx.sidebarCollapsed", collapsed ? "1" : "0");
  }, [collapsed]);

  // Silent update check — reveals the zone above the user widget only if found.
  useEffect(() => {
    if (!isTauri()) return;
    const t = setTimeout(async () => {
      const u = await safeInvoke("check_for_update");
      if (u?.version) setUpdate(u);
    }, 3000);
    return () => clearTimeout(t);
  }, []);

  const linkClass = ({ isActive }) =>
    `group relative flex items-center ${collapsed ? "justify-center" : "gap-3"} px-3 py-2 rounded-lg text-sm transition-colors ${
      isActive
        ? "bg-primary-100 dark:bg-primary-950/40 text-primary-800 dark:text-primary-200"
        : "text-aonyx-600 dark:text-aonyx-400 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 hover:text-aonyx-900 dark:hover:text-aonyx-100"
    }`;

  return (
    <aside
      className={`${collapsed ? "w-16" : "w-60"} flex-shrink-0 flex flex-col bg-aonyx-100 dark:bg-aonyx-950 border-r border-aonyx-200 dark:border-aonyx-800 transition-[width] duration-200`}
    >
      <nav className="flex-1 overflow-y-auto p-2 space-y-1">
        {NAV.map(({ to, label, icon: Icon, end }) => (
          <NavLink key={to} to={to} end={end} title={collapsed ? label : ""} className={linkClass}>
            {({ isActive }) => (
              <>
                {isActive && !collapsed && (
                  <span className="absolute left-0 top-2 bottom-2 w-0.5 rounded-r-full bg-primary-600 dark:bg-primary-400" />
                )}
                <Icon className="w-[18px] h-[18px] flex-shrink-0" strokeWidth={1.75} />
                {!collapsed && <span className="font-medium">{label}</span>}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      {/* Footer: update zone (above) + theme/collapse + user widget (bottom) */}
      <div className="p-2 border-t border-aonyx-200 dark:border-aonyx-800 space-y-1.5">
        {update && (
          <button
            title={`Mise à jour ${update.version} disponible`}
            className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2"} px-3 py-1.5 rounded-md text-emerald-700 dark:text-emerald-400 hover:bg-emerald-50 dark:hover:bg-emerald-950/30 transition-colors`}
          >
            <ArrowUpCircle className="w-4 h-4 flex-shrink-0" strokeWidth={1.75} />
            {!collapsed && <span className="text-xs font-medium truncate">MAJ {update.version}</span>}
          </button>
        )}

        <div className={`flex items-center ${collapsed ? "flex-col gap-1" : "gap-1"}`}>
          <button
            onClick={toggle}
            title={theme === "dark" ? "Thème clair" : "Thème sombre"}
            className="flex items-center justify-center w-8 h-8 rounded-md text-aonyx-500 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 hover:text-aonyx-800 dark:hover:text-aonyx-200 transition-colors"
          >
            {theme === "dark" ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
          </button>
          {!collapsed && <span className="flex-1" />}
          <button
            onClick={() => setCollapsed((c) => !c)}
            title={collapsed ? "Étendre" : "Réduire"}
            className="flex items-center justify-center w-8 h-8 rounded-md text-aonyx-500 hover:bg-aonyx-200/60 dark:hover:bg-aonyx-900/50 hover:text-aonyx-800 dark:hover:text-aonyx-200 transition-colors"
          >
            {collapsed ? <ChevronRight className="w-4 h-4" /> : <ChevronLeft className="w-4 h-4" />}
          </button>
        </div>

        <button
          onClick={() => (isAuthenticated ? logout() : signIn())}
          title={isAuthenticated ? "Se déconnecter" : "Se connecter (aonyx-account)"}
          className={`w-full flex items-center ${collapsed ? "justify-center" : "gap-2.5"} px-2.5 py-2 rounded-lg bg-aonyx-200/50 dark:bg-aonyx-900/60 hover:bg-aonyx-200 dark:hover:bg-aonyx-900 transition-colors`}
        >
          <span className="flex items-center justify-center w-7 h-7 rounded-full bg-aonyx-300 dark:bg-aonyx-800 text-aonyx-700 dark:text-aonyx-200 flex-shrink-0">
            <User className="w-4 h-4" strokeWidth={1.75} />
          </span>
          {!collapsed && (
            <span className="flex flex-col items-start min-w-0 leading-tight">
              <span className="text-sm font-medium text-aonyx-900 dark:text-aonyx-100 truncate">
                {isAuthenticated ? user?.email : "Se connecter"}
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
