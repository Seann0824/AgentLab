import { useEffect, useMemo, useRef } from "react";
import { useChatStore } from "../store/chatStore";
import type { ChatMessage } from "../types/chat";
import { UserMessage, AssistantMessage } from "./MessageItem";
import { ScrollContainer } from "./ScrollContainer";

const EMPTY_MESSAGES: ChatMessage[] = [];

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
  const currentSessionId = useChatStore((s) => s.currentSessionId);
  const messages = useChatStore((s) =>
    currentSessionId ? s.messagesBySession[currentSessionId] ?? EMPTY_MESSAGES : EMPTY_MESSAGES,
  );
  const isStreaming = useChatStore((s) =>
    currentSessionId
      ? (s.streamingBySession[currentSessionId]?.isStreaming ?? false)
      : false,
  );
  const streamingMessageId = useChatStore((s) =>
    currentSessionId
      ? (s.streamingBySession[currentSessionId]?.streamingMessageId ?? null)
      : null,
  );

  const grouped = useMemo(() => groupMessages(messages), [messages]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);

  const scrollToBottom = () => {
    // 用 requestAnimationFrame 确保 DOM 已经完成布局再滚动。
    requestAnimationFrame(() => {
      const bottomEl = bottomRef.current;
      if (bottomEl) {
        bottomEl.scrollIntoView({ block: "end", inline: "nearest" });
        return;
      }
      const el = scrollRef.current;
      if (el) {
        el.scrollTo({ top: el.scrollHeight, behavior: "auto" });
      }
    });
  };

  // 切换 session 时默认滚到底部。
  useEffect(() => {
    isAtBottomRef.current = true;
    scrollToBottom();
  }, [currentSessionId]);

  // 消息变化时，如果用户之前已经在底部，则继续跟随滚底。
  useEffect(() => {
    if (isAtBottomRef.current) {
      scrollToBottom();
    }
  }, [messages, isStreaming]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    isAtBottomRef.current = distanceFromBottom < 32; // 32px 容差
  };

  if (messages.length === 0 && !isStreaming) {
    return (
      <div className="flex-1 flex items-center justify-center text-stone-light text-sm">
        开始一段新的对话…
      </div>
    );
  }

  return (
    <ScrollContainer
      ref={scrollRef}
      onScroll={handleScroll}
      className="flex-1 px-6 py-8"
    >
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
      <div ref={bottomRef} aria-hidden="true" />
    </ScrollContainer>
  );
}
