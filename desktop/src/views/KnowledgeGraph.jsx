import { Database } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import EmptyState from "../components/ui/EmptyState";

export default function KnowledgeGraph() {
  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Database} title="Knowledge Graph" />
      <EmptyState
        icon={Database}
        title="Bientôt"
        note="La visualisation du graphe (cytoscape) nécessite un endpoint KG côté agent (p.ex. /v1/kg). À brancher après le MVP."
      />
    </div>
  );
}
