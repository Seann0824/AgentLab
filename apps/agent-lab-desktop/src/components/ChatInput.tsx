import { useState } from "react";
import { useChatStore } from "../store/chatStore";

export function ChatInput() {
  const [content, setContent] = useState("");
  const isStreaming = useChatStore((s) => s.isStreaming);
  const sendMessage = useChatStore((s) => s.sendMessage);

  const handleSend = async () => {
    const trimmed = content.trim();
    if (!trimmed || isStreaming) return;
    setContent("");
    await sendMessage(trimmed);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="px-6 py-4 border-t border-mist bg-paper">
      <div className="flex items-end gap-3 max-w-4xl mx-auto">
        <textarea
          value={content}
          onChange={(e) => setContent(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
          placeholder="输入消息，Shift + Enter 换行…"
          disabled={isStreaming}
          rows={1}
          className="input-minimal flex-1 max-h-40 resize-none py-3 px-4"
          style={{ minHeight: "48px" }}
        />
        <button
          onClick={handleSend}
          disabled={isStreaming || !content.trim()}
          className="btn-moss px-6 py-3 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {isStreaming ? "思考中" : "发送"}
        </button>
      </div>
    </div>
  );
}
