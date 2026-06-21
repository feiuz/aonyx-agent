import { useEffect, useState } from "react";
import { MessagesSquare, Send, Save, AlertTriangle } from "lucide-react";
import PageHeader from "../components/ui/PageHeader";
import { useI18n } from "../context/LanguageContext";
import { readMessaging, saveMessaging } from "../services/messagingService";

const CHANNELS = [
  { id: "telegram", name: "Telegram", descKey: "messaging.telegramDesc" },
  { id: "discord", name: "Discord", descKey: "messaging.discordDesc" },
];
const ROADMAP = ["Slack", "Matrix", "WhatsApp", "Signal", "Email", "SMS", "Mattermost"];

const inputCls =
  "w-full rounded-lg px-3 py-2 text-sm bg-white dark:bg-aonyx-950 border border-aonyx-300 dark:border-aonyx-700 focus:outline-none focus:border-primary-500 select-text";

function ChannelCard({ ch, data, onSave }) {
  const { t } = useI18n();
  const [token, setToken] = useState("");
  const [allowed, setAllowed] = useState((data.allowed || []).join(", "));
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");

  const save = async () => {
    setBusy(true);
    setMsg("");
    const ids = allowed
      .split(",")
      .map((s) => parseInt(s.trim(), 10))
      .filter((n) => !Number.isNaN(n));
    try {
      await onSave(ch.id, ids, token);
      setToken("");
      setMsg(t("messaging.saved"));
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-xl border border-aonyx-200 dark:border-aonyx-800 p-4 space-y-3">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Send className="w-4 h-4 text-primary-500" />
          <span className="font-medium text-aonyx-900 dark:text-aonyx-50">{ch.name}</span>
        </div>
        <span
          className={`text-[11px] px-2 py-0.5 rounded-full ${
            data.hasToken
              ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
              : "bg-amber-500/15 text-amber-700 dark:text-amber-400"
          }`}
        >
          {data.hasToken ? t("messaging.configured") : t("messaging.needsSetup")}
        </span>
      </div>
      <p className="text-xs text-aonyx-500">{t(ch.descKey)}</p>
      <input
        type="password"
        value={token}
        onChange={(e) => setToken(e.target.value)}
        placeholder={data.hasToken ? t("messaging.tokenSet") : t("messaging.token")}
        className={inputCls}
        spellCheck={false}
      />
      <div>
        <input
          value={allowed}
          onChange={(e) => setAllowed(e.target.value)}
          placeholder={t("messaging.allowed")}
          className={`${inputCls} font-mono`}
          spellCheck={false}
        />
        <p className="text-[11px] text-aonyx-400 mt-1">{t("messaging.allowedHint")}</p>
      </div>
      <div className="flex items-center gap-2">
        <button
          onClick={save}
          disabled={busy}
          className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-primary-600 hover:bg-primary-700 text-white font-medium disabled:opacity-50"
        >
          <Save className="w-4 h-4" /> {t("messaging.save")}
        </button>
        {msg && <span className="text-sm text-aonyx-500 truncate">{msg}</span>}
      </div>
    </div>
  );
}

export default function Messaging() {
  const { t } = useI18n();
  const [data, setData] = useState(null);

  useEffect(() => {
    readMessaging().then(setData);
  }, []);

  const onSave = async (channel, allowed, token) => {
    await saveMessaging(channel, allowed, token);
    setData(await readMessaging());
  };

  return (
    <div className="flex flex-col h-full">
      <PageHeader icon={MessagesSquare} title={t("nav.messaging")} subtitle={t("messaging.subtitle")} />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-3xl space-y-4">
          <div className="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-50/50 dark:bg-amber-950/20 p-3 text-xs text-amber-800 dark:text-amber-300">
            <AlertTriangle className="w-4 h-4 flex-shrink-0 mt-0.5" />
            <span>{t("messaging.note")}</span>
          </div>

          {data &&
            CHANNELS.map((ch) => <ChannelCard key={ch.id} ch={ch} data={data[ch.id]} onSave={onSave} />)}

          <div>
            <p className="text-[11px] uppercase tracking-wide text-aonyx-500 mb-2">{t("messaging.roadmap")}</p>
            <div className="flex flex-wrap gap-2">
              {ROADMAP.map((r) => (
                <span
                  key={r}
                  className="text-xs px-2 py-1 rounded-md border border-aonyx-200 dark:border-aonyx-800 text-aonyx-400"
                >
                  {r}
                </span>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
