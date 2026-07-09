import { useEffect, useRef } from "react";
import { useChatStore } from "../store/chatStore";
import { MessageItem } from "./MessageItem";

export function MessageList() {
  const messages = useChatStore((s) => s.messages);
  const isStreaming = useChatStore((s) => s.isStreaming);
  const streamingMessageId = useChatStore((s) => s.streamingMessageId);
  const bottomRef = useRef<HTMLDivElement>(null);

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
      {messages.map((message) => (
        <MessageItem key={message.id} message={message} />
      ))}
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
