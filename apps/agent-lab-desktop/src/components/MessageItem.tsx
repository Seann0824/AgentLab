import { useState } from "react";
import type { ChatMessage } from "../types/chat";

interface MessageItemProps {
  message: ChatMessage;
}

export function UserMessage({ content }: { content: string }) {
  return (
    <div className="flex justify-end mb-6">
      <div className="max-w-[80%] px-5 py-3 bg-white rounded-2xl rounded-tr-sm text-ink text-sm leading-relaxed shadow-sm">
        {content}
      </div>
    </div>
  );
}

interface AssistantMessageProps {
  message: ChatMessage;
  toolMessages?: ChatMessage[];
}

export function AssistantMessage({ message, toolMessages = [] }: AssistantMessageProps) {
  const [isReasonExpanded, setIsReasonExpanded] = useState(false);
  const reason = message.metadata?.reason as string | undefined;
  const isReasoning = message.metadata?.isReasoning as boolean | undefined;
  const hasContent = message.content.trim().length > 0;
  const hasThinking = reason || isReasoning || toolMessages.length > 0;

  return (
    <div className="flex justify-start mb-6">
      <div className="max-w-[90%] px-5 py-3 bg-paper-dark rounded-2xl rounded-tl-sm text-ink-light text-sm leading-relaxed shadow-sm">
        {hasThinking && (
          <div className="mb-3 pb-3 border-b border-mist">
            <button
              onClick={() => setIsReasonExpanded((v) => !v)}
              className="flex items-center gap-1 text-xs text-stone hover:text-ink-light transition-colors"
            >
              <span
                className={`inline-block transform transition-transform ${
                  isReasonExpanded || isReasoning ? "rotate-90" : ""
                }`}
              >
                ▶
              </span>
              思考过程
              {isReasoning && (
                <span className="ml-1 w-1 h-1 bg-stone rounded-full animate-pulse" />
              )}
            </button>
            {(isReasonExpanded || isReasoning) && (
              <div className="mt-2 space-y-3">
                {(reason || isReasoning) && (
                  <div className="text-stone text-xs leading-relaxed whitespace-pre-wrap">
                    {isReasoning ? message.content : reason}
                  </div>
                )}
                {toolMessages.length > 0 && (
                  <div>
                    <div className="mb-2 text-xs text-stone">已决定调用工具：</div>
                    <div className="space-y-1">
                      {toolMessages.map((tm) => (
                        <ToolCallItem key={tm.tool_call_id ?? tm.id} message={tm} />
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
        {isReasoning ? null : hasContent ? (
          <div className="whitespace-pre-wrap">{message.content}</div>
        ) : toolMessages.length > 0 ? null : (
          <div className="text-stone italic">思考中…</div>
        )}
      </div>
    </div>
  );
}

function ToolCallItem({ message }: { message: ChatMessage }) {
  const [isExpanded, setIsExpanded] = useState(false);
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
        onClick={() => setIsExpanded((v) => !v)}
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
}

export function MessageItem({ message }: MessageItemProps) {
  switch (message.role) {
    case "user":
      return <UserMessage content={message.content} />;
    case "assistant":
      return <AssistantMessage message={message} />;
    default:
      return null;
  }
}
