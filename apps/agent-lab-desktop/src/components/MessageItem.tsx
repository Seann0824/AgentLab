import { useState } from "react";
import type { ChatMessage } from "../types/chat";

interface MessageItemProps {
  message: ChatMessage;
}

function UserMessage({ content }: { content: string }) {
  return (
    <div className="flex justify-end mb-6">
      <div className="max-w-[80%] px-5 py-3 bg-white rounded-2xl rounded-tr-sm text-ink text-sm leading-relaxed shadow-sm">
        {content}
      </div>
    </div>
  );
}

function AssistantMessage({ message }: { message: ChatMessage }) {
  const [isReasonExpanded, setIsReasonExpanded] = useState(false);
  const reason = message.metadata?.reason as string | undefined;
  const isReasoning = message.metadata?.isReasoning as boolean | undefined;

  return (
    <div className="flex justify-start mb-6">
      <div className="max-w-[90%] px-5 py-3 bg-paper-dark rounded-2xl rounded-tl-sm text-ink-light text-sm leading-relaxed shadow-sm">
        {(reason || isReasoning) && (
          <div className="mb-3 pb-3 border-b border-mist">
            <button
              onClick={() => setIsReasonExpanded((v) => !v)}
              className="flex items-center gap-1 text-xs text-stone hover:text-ink-light transition-colors"
            >
              <span
                className={`transform transition-transform ${
                  isReasonExpanded ? "rotate-90" : ""
                }`}
              >
                {isReasonExpanded || isReasoning ? "▼" : "▶"}
              </span>
              思考过程
              {isReasoning && (
                <span className="ml-1 w-1 h-1 bg-stone rounded-full animate-pulse" />
              )}
            </button>
            {(isReasonExpanded || isReasoning) && (
              <div className="mt-2 text-stone text-xs leading-relaxed whitespace-pre-wrap">
                {isReasoning ? message.content : reason}
              </div>
            )}
          </div>
        )}
        {isReasoning ? null : message.content ? (
          <div className="whitespace-pre-wrap">{message.content}</div>
        ) : message.tool_calls && message.tool_calls.length > 0 ? (
          <div className="text-stone italic">正在决定下一步…</div>
        ) : (
          <div className="text-stone italic">思考中…</div>
        )}
      </div>
    </div>
  );
}

function ToolMessage({ message }: { message: ChatMessage }) {
  const status = message.metadata?.status as string | undefined;
  const toolName = (message.metadata?.tool_name as string) ?? "工具";
  const args = message.metadata?.arguments as string | undefined;

  const isRunning = status === "running";
  const isError = status === "error";

  return (
    <div className="flex justify-start mb-6 pl-4">
      <div
        className={`
          max-w-[90%] px-4 py-3 rounded-lg text-xs leading-relaxed border
          ${isError ? "border-red-200 bg-red-50 text-red-800" : "border-mist bg-paper-dark text-ink-light"}
        `}
      >
        <div className="flex items-center gap-2 mb-2">
          <span className="font-medium">{toolName}</span>
          {isRunning && <span className="text-stone animate-pulse">执行中…</span>}
          {isError && <span className="text-red-600">错误</span>}
          {!isRunning && !isError && <span className="text-stone">已完成</span>}
        </div>
        {args && (
          <pre className="text-[11px] text-stone mb-2 whitespace-pre-wrap break-all">
            {args}
          </pre>
        )}
        {!isRunning && message.content && (
          <pre className="text-[11px] whitespace-pre-wrap break-all">
            {message.content}
          </pre>
        )}
      </div>
    </div>
  );
}

export function MessageItem({ message }: MessageItemProps) {
  switch (message.role) {
    case "user":
      return <UserMessage content={message.content} />;
    case "assistant":
      return <AssistantMessage message={message} />;
    case "tool":
      return <ToolMessage message={message} />;
    default:
      return null;
  }
}
