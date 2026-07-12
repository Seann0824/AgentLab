import { useEffect, useState } from "react";
import { useChatStore } from "../../store/chatStore";
import type { SessionSummary } from "../../types/chat";
import { ConfirmDialog } from "../ConfirmDialog";
import { NewChatButton } from "../NewChatButton";
import { ScrollContainer } from "../ScrollContainer";
import { SessionItem } from "./SessionItem";

export function Sidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const {
    sessions,
    currentSessionId,
    streamingBySession,
    unreadBySession,
    loadSessions,
    selectSession,
    deleteSession,
    renameSession,
  } = useChatStore();
  const [deletingSession, setDeletingSession] = useState<SessionSummary | null>(null);

  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

  const handleConfirmDelete = () => {
    if (deletingSession) {
      deleteSession(deletingSession.id);
    }
    setDeletingSession(null);
  };

  return (
    <aside className="w-64 flex flex-col bg-paper-dark border-r border-mist flex-shrink-0">
      <div className="p-4 border-b border-mist">
        <NewChatButton />
      </div>
      <ScrollContainer className="flex-1 py-2">
        {sessions.map((session) => (
          <SessionItem
            key={session.id}
            session={session}
            isActive={session.id === currentSessionId}
            isStreaming={streamingBySession[session.id]?.isStreaming ?? false}
            unreadCount={unreadBySession[session.id] ?? 0}
            onSelect={() => selectSession(session.id)}
            onRename={(title) => renameSession(session.id, title)}
            onDelete={() => setDeletingSession(session)}
          />
        ))}
      </ScrollContainer>

      <div className="p-4 border-t border-mist">
        <button
          onClick={onOpenSettings}
          className="w-full text-left text-sm text-stone hover:text-ink transition-colors"
        >
          设置
        </button>
      </div>

      <ConfirmDialog
        isOpen={deletingSession !== null}
        title="删除会话"
        message={
          deletingSession
            ? `确定删除会话「${deletingSession.title}」吗？删除后无法恢复。`
            : ""
        }
        confirmText="删除"
        onConfirm={handleConfirmDelete}
        onCancel={() => setDeletingSession(null)}
      />
    </aside>
  );
}
