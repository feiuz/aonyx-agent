import { useEffect, useMemo, useState } from "react";
import CytoscapeComponent from "react-cytoscapejs";
import { Database } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import EmptyState from "../components/ui/EmptyState";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import * as agent from "../services/agentService";

const STYLESHEET = [
  {
    selector: "node",
    style: {
      "background-color": "#f95416",
      label: "data(label)",
      color: "#a1a1aa",
      "font-size": 9,
      "font-family": "Saira, sans-serif",
      "text-valign": "bottom",
      "text-margin-y": 3,
      width: 14,
      height: 14,
    },
  },
  {
    selector: "edge",
    style: {
      width: 1,
      "line-color": "#43444a",
      "target-arrow-color": "#43444a",
      "target-arrow-shape": "triangle",
      "curve-style": "bezier",
      label: "data(label)",
      "font-size": 7,
      color: "#86878f",
      "text-rotation": "autorotate",
    },
  },
];

export default function KnowledgeGraph() {
  const { status } = useAgent();
  const { t } = useI18n();
  const [entities, setEntities] = useState([]);
  const [relations, setRelations] = useState([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState("");

  useEffect(() => {
    if (status !== "ok") return;
    let cancel = false;
    (async () => {
      setLoading(true);
      setErr("");
      try {
        const [ents, rels] = await Promise.all([agent.kgEntities(300), agent.kgRelations(800)]);
        if (cancel) return;
        setEntities(Array.isArray(ents) ? ents : []);
        setRelations(Array.isArray(rels) ? rels : []);
      } catch (e) {
        if (!cancel) setErr(String(e));
      } finally {
        if (!cancel) setLoading(false);
      }
    })();
    return () => {
      cancel = true;
    };
  }, [status]);

  const elements = useMemo(() => {
    const ids = new Set(entities.map((e) => e.id));
    const nodes = entities.map((e) => ({ data: { id: e.id, label: e.name, type: e.entity_type || "" } }));
    const edges = relations
      .filter((r) => ids.has(r.src_id) && ids.has(r.dst_id))
      .map((r) => ({ data: { id: r.id, source: r.src_id, target: r.dst_id, label: r.predicate || "" } }));
    return [...nodes, ...edges];
  }, [entities, relations]);

  const subtitle =
    status === "ok" ? `${entities.length} ${t("kg.nodes")} · ${relations.length} ${t("kg.edges")}` : "";

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={Database} title={t("nav.kg")} subtitle={subtitle} />
      {status !== "ok" ? (
        <EmptyState icon={Database} note={t("common.connect")} />
      ) : err ? (
        <EmptyState icon={Database} note={err} />
      ) : entities.length === 0 ? (
        <EmptyState icon={Database} note={loading ? "…" : t("kg.empty")} />
      ) : (
        <div className="flex-1 min-h-0">
          <CytoscapeComponent
            elements={elements}
            stylesheet={STYLESHEET}
            layout={{ name: "cose", animate: false, padding: 30, nodeRepulsion: 6000 }}
            style={{ width: "100%", height: "100%" }}
          />
        </div>
      )}
    </div>
  );
}
