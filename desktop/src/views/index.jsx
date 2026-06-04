import { LayoutDashboard, Users as UsersIcon, Shield, Download } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import EmptyState from "../components/ui/EmptyState";
import { useI18n } from "../context/LanguageContext";

// Shared placeholder for sections not yet built out.
function Stub({ icon, titleKey, noteKey }) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={icon} title={t(titleKey)} />
      <EmptyState icon={icon} note={t(noteKey)} />
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
export const Dashboard = () => <Stub icon={LayoutDashboard} titleKey="nav.dashboard" noteKey="stub.dashboard" />;
export const Users = () => <Stub icon={UsersIcon} titleKey="nav.users" noteKey="stub.users" />;
export const Permissions = () => <Stub icon={Shield} titleKey="nav.permissions" noteKey="stub.permissions" />;
export const Mcp = () => <Stub icon={Download} titleKey="nav.mcp" noteKey="stub.mcp" />;
