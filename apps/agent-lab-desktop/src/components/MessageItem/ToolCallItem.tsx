import { memo } from "react";
import type { ChatMessage } from "../../types/chat";
import { useExpandable } from "./useExpandable";

export const ToolCallItem = memo(function ToolCallItem({
  message,
}: {
  message: ChatMessage;
}) {
  const { isExpanded, toggle } = useExpandable();
  const status = message.metadata?.status as string | undefined;
  const toolName = (message.metadata?.tool_name as string) ?? "工具";
  const args = message.metadata?.arguments as string | undefined;

  const isRunning = status === "running";
  const isError = status === "error";

  return (
    <div
      className={`
        rounded text-xs leading-relaxed border overflow-hidden
        ${isError ? "border-red-200 bg-red-50 text-red-800" : "border-mist bg-paper-dark text-ink-light"}
      `}
    >
      <button
        onClick={toggle}
        className="w-full px-3 py-2 flex items-center justify-between gap-3 hover:bg-black/[0.02] transition-colors text-left"
      >
        <div className="flex items-center gap-2">
          <span
            className={`inline-block transform transition-transform ${
              isExpanded ? "rotate-90" : ""
            }`}
          >
            ▶
          </span>
          <span className="font-medium">{toolName}</span>
        </div>
        <div>
          {isRunning && <span className="text-stone animate-pulse">执行中…</span>}
          {isError && <span className="text-red-600">错误</span>}
          {!isRunning && !isError && <span className="text-stone">已完成</span>}
        </div>
      </button>
      {isExpanded && (
        <div className="px-3 pb-2 border-t border-mist/50">
          {args && (
            <pre className="pt-2 text-[11px] text-stone mb-2 whitespace-pre-wrap break-all">
              {args}
            </pre>
          )}
          {!isRunning && message.content && (
            <pre className="text-[11px] whitespace-pre-wrap break-all">
              {message.content}
            </pre>
          )}
        </div>
      )}
    </div>
  );
});
