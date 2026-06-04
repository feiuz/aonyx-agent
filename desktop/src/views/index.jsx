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
} from "lucide-react";
import PageHeader from "../components/ui/PageHeader";

// P0 placeholders — each section gets its real view in later phases.
function Stub({ icon: Icon, title, note }) {
  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Icon} title={title} />
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center text-aonyx-500">
          <Icon className="w-12 h-12 mx-auto opacity-25" strokeWidth={1.5} />
          <p className="mt-3 text-sm">{note || "Section en construction."}</p>
        </div>
      </div>
    </div>
  );
}

export { default as Chat } from "./Chat";
export const Dashboard = () => <Stub icon={LayoutDashboard} title="Dashboard" note="Vue d'ensemble — à venir." />;
export const Projets = () => <Stub icon={FolderOpen} title="Projets" />;
export const Stats = () => <Stub icon={BarChart3} title="Statistiques" />;
export const MemoryHealth = () => <Stub icon={Activity} title="Memory Health" />;
export const KnowledgeGraph = () => <Stub icon={Database} title="Knowledge Graph" note="Graphe (cytoscape) — à venir." />;
export const Users = () => <Stub icon={UsersIcon} title="Utilisateurs" />;
export const Permissions = () => <Stub icon={Shield} title="Permissions" />;
export const Mcp = () => <Stub icon={Download} title="MCP" note="Serveurs MCP — à venir." />;
export const Settings = () => <Stub icon={SettingsIcon} title="Paramètres" note="Provider / LLM / RAG / Embeddings — P2." />;
