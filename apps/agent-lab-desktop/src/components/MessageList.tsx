import { useEffect, useRef } from "react";
import { useChatStore } from "../store/chatStore";
import { MessageItem } from "./MessageItem";

export function MessageList() {
  const messages = useChatStore((s) => s.messages);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  if (messages.length === 0) {
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
      <div ref={bottomRef} />
    </div>
  );
}
