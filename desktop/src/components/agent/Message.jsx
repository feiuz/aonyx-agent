import Markdown from "./Markdown";
import { useI18n } from "../../context/LanguageContext";

export default function Message({ role, content, tools, error, streaming }) {
  const { t } = useI18n();
  const isUser = role === "user";
  return (
    <div className={`flex flex-col gap-1 max-w-[80%] ${isUser ? "self-end items-end" : "self-start"}`}>
      <span className="text-[11px] font-cond uppercase tracking-wider text-aonyx-500">
        {isUser ? t("chat.you") : "aonyx"}
      </span>
      <div
        className={`rounded-lg px-3.5 py-2.5 border select-text break-words ${
          error
            ? "border-red-500 text-red-500"
            : isUser
              ? "bg-aonyx-200/50 dark:bg-aonyx-800/60 border-aonyx-300 dark:border-aonyx-700"
              : "bg-aonyx-100 dark:bg-aonyx-950 border-aonyx-200 dark:border-aonyx-800"
        } ${streaming ? "italic text-aonyx-500" : ""}`}
      >
        {isUser ? (
          <span className="whitespace-pre-wrap">{content}</span>
        ) : (
          <Markdown>{content}</Markdown>
        )}
      </div>
      {tools?.length > 0 && (
        <div className="flex items-center gap-1.5 text-xs font-mono text-aonyx-500">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
          {tools.join(", ")}
        </div>
      )}
    </div>
  );
}
