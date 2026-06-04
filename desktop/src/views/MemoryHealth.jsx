import { useState } from "react";
import { Activity, Search } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useAgent } from "../context/AgentContext";
import * as agent from "../services/agentService";

export default function MemoryHealth() {
  const { status } = useAgent();
  const [q, setQ] = useState("");
  const [hits, setHits] = useState(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState("");

  const search = async () => {
    if (!q.trim() || status !== "ok" || loading) return;
    setLoading(true);
    setErr("");
    try {
      const h = await agent.memorySearch(q.trim(), 12);
      setHits(Array.isArray(h) ? h : []);
    } catch (e) {
      setErr(String(e));
      setHits([]);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        icon={Activity}
        title="Memory Health"
        subtitle="Recherche hybride dans le palais de mémoire (BM25 + vecteurs · RRF)"
      />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-3xl space-y-4">
          <div className="flex gap-2">
            <input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && search()}
              disabled={status !== "ok"}
              placeholder={status === "ok" ? "Cherche dans la mémoire…" : "Connecte-toi (Paramètres) pour chercher"}
              className="flex-1 rounded-lg px-3 py-2 text-sm bg-white dark:bg-aonyx-950 border border-aonyx-300 dark:border-aonyx-700 focus:outline-none focus:border-primary-500 select-text disabled:opacity-50"
            />
            <button
              onClick={search}
              disabled={loading || status !== "ok" || !q.trim()}
              className="flex items-center justify-center px-4 rounded-lg bg-primary-600 hover:bg-primary-700 text-white disabled:opacity-40"
              aria-label="Chercher"
            >
              <Search className="w-4 h-4" />
            </button>
          </div>

          {err && <p className="text-sm text-red-500 break-words">{err}</p>}

          {hits === null ? (
            <p className="text-sm text-aonyx-500">Lance une recherche pour voir les passages les plus proches.</p>
          ) : hits.length === 0 ? (
            <p className="text-sm text-aonyx-500">{loading ? "recherche…" : "Aucun résultat."}</p>
          ) : (
            <ul className="space-y-2">
              {hits.map((h, i) => (
                <li key={i} className="rounded-lg border border-aonyx-200 dark:border-aonyx-800 p-3 select-text">
                  <div className="flex items-center gap-2 mb-1 text-xs font-mono">
                    <span className="text-emerald-600 dark:text-emerald-400">{(h.score ?? 0).toFixed(3)}</span>
                    {h.project && <span className="text-aonyx-500">· {h.project}</span>}
                    {h.source && <span className="text-aonyx-500 truncate">· {h.source}</span>}
                  </div>
                  <p className="text-sm text-aonyx-700 dark:text-aonyx-300 whitespace-pre-wrap">
                    {(h.content || "").slice(0, 400)}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
