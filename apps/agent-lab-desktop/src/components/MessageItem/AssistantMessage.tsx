import { memo } from "react";
import type { ChatMessage } from "../../types/chat";
import { MarkdownRenderer } from "../MarkdownRenderer";
import { ToolCallItem } from "./ToolCallItem";
import { useExpandable } from "./useExpandable";

export interface AssistantMessageProps {
  message: ChatMessage;
  toolMessages?: ChatMessage[];
}

function AssistantMessageRaw({ message, toolMessages = [] }: AssistantMessageProps) {
  const reasonExpand = useExpandable();
  const toolsExpand = useExpandable();

  const reason = message.metadata?.reason as string | undefined;
  const isReasoning = message.metadata?.isReasoning as boolean | undefined;
  const hasContent = message.content.trim().length > 0;
  const hasToolCalls = toolMessages.length > 0;

  return (
    <div className="flex justify-start mb-6">
      <div className="w-[90%] px-5 py-3 bg-paper-dark rounded-2xl rounded-tl-sm text-ink-light text-sm leading-relaxed shadow-sm">
        {(reason || isReasoning) && (
          <div className="mb-3 pb-3 border-b border-mist">
            <button
              onClick={reasonExpand.toggle}
              className="flex items-center gap-1 text-xs text-stone hover:text-ink-light transition-colors"
            >
              <span
                className={`inline-block transform transition-transform ${
                  reasonExpand.isExpanded || isReasoning ? "rotate-90" : ""
                }`}
              >
                ▶
              </span>
              思考过程
              {isReasoning && (
                <span className="ml-1 w-1 h-1 bg-stone rounded-full animate-pulse" />
              )}
            </button>
            {(reasonExpand.isExpanded || isReasoning) && reason && (
              <div className="mt-2 text-stone text-xs leading-relaxed whitespace-pre-wrap">
                {isReasoning ? message.content : reason}
              </div>
            )}
          </div>
        )}
        {isReasoning ? null : hasContent ? (
          <MarkdownRenderer content={message.content} />
        ) : (
          <div className="text-stone italic">思考中…</div>
        )}

        {!isReasoning && hasToolCalls && (
          <div className="mt-3 pt-3 border-t border-mist">
            <button
              onClick={toolsExpand.toggle}
              className="flex items-center gap-1 text-xs text-stone hover:text-ink-light transition-colors"
            >
              <span
                className={`inline-block transform transition-transform ${
                  toolsExpand.isExpanded ? "rotate-90" : ""
                }`}
              >
                ▶
              </span>
              查看工具调用 ({toolMessages.length})
            </button>
            {toolsExpand.isExpanded && (
              <div className="mt-2 space-y-1">
                {toolMessages.map((tm) => (
                  <ToolCallItem key={tm.tool_call_id ?? tm.id} message={tm} />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function assistantMessageEqual(
  prev: AssistantMessageProps,
  next: AssistantMessageProps,
): boolean {
  if (prev.message !== next.message) return false;
  const prevTools = prev.toolMessages ?? [];
  const nextTools = next.toolMessages ?? [];
  if (prevTools.length !== nextTools.length) return false;
  return prevTools.every((tm, i) => tm === nextTools[i]);
}

export const AssistantMessage = memo(AssistantMessageRaw, assistantMessageEqual);
