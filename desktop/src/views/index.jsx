import { LayoutDashboard, Users as UsersIcon, Shield, Download } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import EmptyState from "../components/ui/EmptyState";

// Shared placeholder for sections not yet built out.
function Stub({ icon, title, note }) {
  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={icon} title={title} />
      <EmptyState icon={icon} note={note || "Section en construction."} />
    </div>
  );
}

// Real views (their own files)
export { default as Chat } from "./Chat";
export { default as Settings } from "./Settings";
export { default as MemoryHealth } from "./MemoryHealth";
export { default as KnowledgeGraph } from "./KnowledgeGraph";
export { default as Projets } from "./Projets";
export { default as Stats } from "./Stats";

// Placeholders
export const Dashboard = () => <Stub icon={LayoutDashboard} title="Dashboard" note="Vue d'ensemble — à venir." />;
export const Users = () => (
  <Stub icon={UsersIcon} title="Utilisateurs" note="Multi-utilisateurs — réservé à aonyx-account (à confirmer, OQ-c)." />
);
export const Permissions = () => (
  <Stub icon={Shield} title="Permissions" note="Réservé à aonyx-account (à confirmer, OQ-c)." />
);
export const Mcp = () => <Stub icon={Download} title="MCP" note="Gestion des serveurs MCP — à venir." />;
