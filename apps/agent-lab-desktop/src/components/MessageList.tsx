import { useEffect, useRef } from "react";
import { useChatStore } from "../store/chatStore";
import type { ChatMessage } from "../types/chat";
import { UserMessage, AssistantMessage } from "./MessageItem";

interface MessageGroup {
  type: "user" | "assistant";
  message: ChatMessage;
  toolMessages: ChatMessage[];
}

function groupMessages(messages: ChatMessage[]): MessageGroup[] {
  const groups: MessageGroup[] = [];
  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    if (msg.role === "user") {
      groups.push({ type: "user", message: msg, toolMessages: [] });
    } else if (msg.role === "assistant") {
      const toolMessages: ChatMessage[] = [];
      let j = i + 1;
      while (j < messages.length && messages[j].role === "tool") {
        toolMessages.push(messages[j]);
        j++;
      }
      groups.push({ type: "assistant", message: msg, toolMessages });
      i = j - 1;
    }
  }
  return groups;
}

export function MessageList() {
  const messages = useChatStore((s) => s.messages);
  const isStreaming = useChatStore((s) => s.isStreaming);
  const streamingMessageId = useChatStore((s) => s.streamingMessageId);
  const bottomRef = useRef<HTMLDivElement>(null);

  const grouped = groupMessages(messages);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, isStreaming, streamingMessageId]);

  if (messages.length === 0 && !isStreaming) {
    return (
      <div className="flex-1 flex items-center justify-center text-stone-light text-sm">
        开始一段新的对话…
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto px-6 py-8">
      {grouped.map((group) =>
        group.type === "user" ? (
          <UserMessage key={group.message.id} content={group.message.content} />
        ) : (
          <AssistantMessage
            key={group.message.id}
            message={group.message}
            toolMessages={group.toolMessages}
          />
        ),
      )}
      {isStreaming && !streamingMessageId && (
        <div className="flex justify-start mb-6">
          <div className="max-w-[90%] px-5 py-3 bg-paper-dark rounded-2xl rounded-tl-sm text-stone text-sm leading-relaxed shadow-sm">
            <span className="inline-flex items-center gap-2">
              <span className="w-1.5 h-1.5 bg-stone rounded-full animate-pulse" />
              思考中…
            </span>
          </div>
        </div>
      )}
      <div ref={bottomRef} />
    </div>
  );
}
