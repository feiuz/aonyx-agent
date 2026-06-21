import { useRef, useState } from "react";
import { Activity, Search, Upload, FileText } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import * as agent from "../services/agentService";

export default function MemoryHealth() {
  const { status } = useAgent();
  const { t } = useI18n();
  const [q, setQ] = useState("");
  const [hits, setHits] = useState(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState("");

  const [source, setSource] = useState("");
  const [text, setText] = useState("");
  const [ingesting, setIngesting] = useState(false);
  const [ingestMsg, setIngestMsg] = useState("");
  const fileRef = useRef(null);

  const onFile = (e) => {
    const file = e.target.files?.[0];
    if (!file) return;
    if (!source.trim()) setSource(file.name);
    const reader = new FileReader();
    reader.onload = () => setText(String(reader.result || ""));
    reader.readAsText(file);
    e.target.value = "";
  };

  const doIngest = async () => {
    if (!text.trim() || status !== "ok" || ingesting) return;
    setIngesting(true);
    setIngestMsg("");
    try {
      const r = await agent.ingest(source.trim() || "uploaded", text.trim());
      setIngestMsg(`✓ ${r?.chunks ?? 0} ${t("memory.ingestedChunks")}`);
      setText("");
    } catch (e) {
      setIngestMsg(String(e));
    } finally {
      setIngesting(false);
    }
  };

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
      <PageHeader icon={Activity} title={t("nav.memory")} subtitle={t("memory.subtitle")} />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-3xl space-y-4">
          <section className="rounded-xl border border-aonyx-200 dark:border-aonyx-800 p-4 space-y-3">
            <div className="flex items-center gap-2">
              <Upload className="w-4 h-4 text-primary-500" />
              <h2 className="font-cond uppercase tracking-wide text-sm text-aonyx-700 dark:text-aonyx-300">{t("memory.ingestTitle")}</h2>
            </div>
            <input
              value={source}
              onChange={(e) => setSource(e.target.value)}
              disabled={status !== "ok"}
              placeholder={t("memory.ingestSource")}
              className="w-full rounded-lg px-3 py-2 text-sm bg-white dark:bg-aonyx-950 border border-aonyx-300 dark:border-aonyx-700 focus:outline-none focus:border-primary-500 select-text disabled:opacity-50"
            />
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              disabled={status !== "ok"}
              rows={5}
              placeholder={t("memory.ingestText")}
              className="w-full rounded-lg px-3 py-2 text-sm bg-white dark:bg-aonyx-950 border border-aonyx-300 dark:border-aonyx-700 focus:outline-none focus:border-primary-500 select-text resize-y disabled:opacity-50"
            />
            <input ref={fileRef} type="file" accept=".md,.markdown,.txt,.json,.csv,text/*" className="hidden" onChange={onFile} />
            <div className="flex items-center gap-2">
              <button
                onClick={() => fileRef.current?.click()}
                disabled={status !== "ok"}
                className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border border-aonyx-300 dark:border-aonyx-700 text-sm hover:bg-aonyx-100 dark:hover:bg-aonyx-900 disabled:opacity-40"
              >
                <FileText className="w-4 h-4" /> {t("memory.ingestFile")}
              </button>
              <button
                onClick={doIngest}
                disabled={ingesting || status !== "ok" || !text.trim()}
                className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-primary-600 hover:bg-primary-700 text-white font-medium disabled:opacity-40"
              >
                <Upload className="w-4 h-4" /> {ingesting ? t("memory.ingesting") : t("memory.ingestBtn")}
              </button>
              {ingestMsg && <span className="text-sm text-aonyx-500 truncate">{ingestMsg}</span>}
            </div>
          </section>

          <div className="flex gap-2">
            <input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && search()}
              disabled={status !== "ok"}
              placeholder={status === "ok" ? t("memory.search") : t("memory.connect")}
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
            <p className="text-sm text-aonyx-500">{t("memory.hint")}</p>
          ) : hits.length === 0 ? (
            <p className="text-sm text-aonyx-500">{loading ? t("memory.searching") : t("memory.none")}</p>
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
