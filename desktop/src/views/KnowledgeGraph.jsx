import { Database } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import EmptyState from "../components/ui/EmptyState";
import { useI18n } from "../context/LanguageContext";

export default function KnowledgeGraph() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Database} title={t("nav.kg")} />
      <EmptyState icon={Database} title={t("kg.soon")} note={t("kg.note")} />
    </div>
  );
}
