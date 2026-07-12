import { useEffect, useState } from "react";
import { useChatStore } from "../../store/chatStore";
import type { SessionSummary } from "../../types/chat";
import { ConfirmDialog } from "../ConfirmDialog";
import { NewChatButton } from "../NewChatButton";
import { ScrollContainer } from "../ScrollContainer";
import { SessionItem } from "./SessionItem";

function CollapseButton({ collapsed }: { collapsed: boolean }) {
  const toggleSidebar = useChatStore((s) => s.toggleSidebar);
  return (
    <button
      onClick={toggleSidebar}
      title={collapsed ? "展开侧边栏" : "收起侧边栏"}
      className="p-2 text-stone hover:text-ink transition-colors rounded-sm hover:bg-paper-dark"
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        className={`transition-transform ${collapsed ? "rotate-180" : ""}`}
      >
        <polyline points="15 18 9 12 15 6" />
      </svg>
    </button>
  );
}

export function Sidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const {
    sessions,
    currentSessionId,
    streamingBySession,
    unreadBySession,
    isSidebarCollapsed,
    loadSessions,
    selectSession,
    deleteSession,
    renameSession,
    startNewChat,
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

  if (isSidebarCollapsed) {
    return (
      <aside className="w-14 flex flex-col bg-paper-dark border-r border-mist flex-shrink-0">
        <div className="h-14 flex items-center justify-center border-b border-mist">
          <CollapseButton collapsed />
        </div>
        <div className="p-2 border-b border-mist">
          <button
            onClick={startNewChat}
            title="新会话"
            className="w-10 h-10 flex items-center justify-center text-paper bg-moss rounded-sm hover:bg-moss/90 transition-colors"
          >
            <span className="text-lg leading-none">+</span>
          </button>
        </div>
        <div className="flex-1" />
        <div className="p-2 border-t border-mist">
          <button
            onClick={onOpenSettings}
            title="设置"
            className="w-10 h-10 flex items-center justify-center text-stone hover:text-ink transition-colors rounded-sm hover:bg-paper-dark"
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
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

  return (
    <aside className="w-64 flex flex-col bg-paper-dark border-r border-mist flex-shrink-0">
      <div className="h-14 flex items-center justify-between px-4 border-b border-mist">
        <span className="text-sm font-medium text-ink">会话</span>
        <CollapseButton collapsed={false} />
      </div>
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
            collapsed={false}
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
