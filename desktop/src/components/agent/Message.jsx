import Markdown from "./Markdown";
import { useI18n } from "../../context/LanguageContext";

export default function Message({ role, content, tools, error, streaming }) {
  const { t } = useI18n();
  const isUser = role === "user";
  return (
    <div className={`flex flex-col gap-1.5 ${isUser ? "self-end items-end max-w-[80%]" : "self-start w-full max-w-[88%]"}`}>
      <span className="text-[11px] font-cond uppercase tracking-wider text-aonyx-500">
        {isUser ? t("chat.you") : "aonyx"}
      </span>
      {isUser ? (
        <div
          className={`rounded-2xl rounded-tr-md px-4 py-2.5 text-sm select-text break-words whitespace-pre-wrap ${
            error
              ? "text-red-500 bg-red-500/10"
              : "bg-aonyx-200/60 dark:bg-aonyx-800/50 text-aonyx-900 dark:text-aonyx-50"
          }`}
        >
          {content}
        </div>
      ) : (
        <div
          className={`text-sm leading-relaxed select-text break-words ${
            error ? "text-red-500" : streaming ? "text-aonyx-500" : "text-aonyx-800 dark:text-aonyx-100"
          }`}
        >
          <Markdown>{content}</Markdown>
        </div>
      )}
      {tools?.length > 0 && (
        <div className="flex items-center gap-1.5 text-[11px] font-mono text-aonyx-500">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
          {tools.join(", ")}
        </div>
      )}
    </div>
  );
}
