import { useState } from "react";
import { useChatStore } from "../store/chatStore";

export function ChatHeader() {
  const { sessions, currentSessionId, renameSession, deleteSession } = useChatStore();
  const currentSession = sessions.find((s) => s.id === currentSessionId);

  const [isEditing, setIsEditing] = useState(false);
  const [title, setTitle] = useState(currentSession?.title ?? "");

  const handleRename = () => {
    const trimmed = title.trim();
    if (currentSessionId && trimmed && trimmed !== currentSession?.title) {
      renameSession(currentSessionId, trimmed);
    }
    setIsEditing(false);
  };

  if (!currentSession) {
    return (
      <header className="h-14 flex items-center px-6 border-b border-mist bg-paper">
        <span className="text-sm text-stone">选择一个会话开始对话</span>
      </header>
    );
  }

  return (
    <header className="h-14 flex items-center justify-between px-6 border-b border-mist bg-paper">
      {isEditing ? (
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.currentTarget.value)}
          onBlur={handleRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleRename();
            if (e.key === "Escape") setIsEditing(false);
          }}
          className="text-base font-medium bg-transparent border-b border-moss outline-none text-ink"
        />
      ) : (
        <h2
          onClick={() => {
            setTitle(currentSession.title);
            setIsEditing(true);
          }}
          className="text-base font-medium text-ink cursor-pointer hover:text-moss transition-colors"
        >
          {currentSession.title}
        </h2>
      )}

      <button
        onClick={() => {
          if (confirm(`确定删除会话「${currentSession.title}」吗？`)) {
            deleteSession(currentSession.id);
          }
        }}
        className="text-xs text-stone hover:text-red-600 transition-colors"
      >
        删除
      </button>
    </header>
  );
}
