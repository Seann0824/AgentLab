import { useEffect } from "react";
import { useChatStore } from "../../store/chatStore";
import type { ChatMessage } from "../../types/chat";
import { AssistantMessage, UserMessage } from "../MessageItem";
import { ScrollContainer } from "../ScrollContainer";
import { useAutoScroll } from "./useAutoScroll";
import { useMessageGroups } from "./useMessageGroups";

const EMPTY_MESSAGES: ChatMessage[] = [];

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

  const grouped = useMessageGroups(messages);
  const { scrollRef, bottomRef, handleScroll, scrollToBottom, tryScrollToBottom } =
    useAutoScroll();

  // 切换 session 时默认滚到底部。
  useEffect(() => {
    scrollToBottom();
  }, [currentSessionId, scrollToBottom]);

  // 消息变化时，如果用户之前已经在底部，则继续跟随滚底。
  useEffect(() => {
    tryScrollToBottom();
  }, [messages, isStreaming, tryScrollToBottom]);

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
          <UserMessage key={group.message.id} message={group.message} />
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
