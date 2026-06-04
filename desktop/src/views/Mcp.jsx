import { useEffect, useState } from "react";
import { Download } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useAgent } from "../context/AgentContext";
import { useI18n } from "../context/LanguageContext";
import * as agent from "../services/agentService";

export default function Mcp() {
  const { status } = useAgent();
  const { t } = useI18n();
  const [tools, setTools] = useState([]);
  const [err, setErr] = useState("");

  useEffect(() => {
    if (status !== "ok") return;
    let cancel = false;
    (async () => {
      try {
        const r = await agent.tools();
        if (!cancel) setTools(Array.isArray(r) ? r : []);
      } catch (e) {
        if (!cancel) setErr(String(e));
      }
    })();
    return () => {
      cancel = true;
    };
  }, [status]);

  return (
    <div className="flex flex-col h-full">
      <PageHeader
        icon={Download}
        title={t("nav.mcp")}
        subtitle={status === "ok" ? `${tools.length} ${t("mcp.tools")}` : ""}
      />
      <div className="flex-1 overflow-y-auto p-6">
        {status !== "ok" ? (
          <p className="text-sm text-aonyx-500">{t("common.connect")}</p>
        ) : err ? (
          <p className="text-sm text-red-500 break-words">{err}</p>
        ) : tools.length === 0 ? (
          <p className="text-sm text-aonyx-500">{t("mcp.none")}</p>
        ) : (
          <ul className="max-w-3xl grid gap-2">
            {tools.map((tl, i) => (
              <li key={i} className="rounded-lg border border-aonyx-200 dark:border-aonyx-800 px-4 py-3">
                <div className="font-mono text-sm text-aonyx-800 dark:text-aonyx-200">{tl.name || tl.id}</div>
                {tl.description && <div className="text-xs text-aonyx-500 mt-0.5">{tl.description}</div>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
