import { useEffect, useState } from "react";
import { useChatStore } from "../store/chatStore";
import { ConfirmDialog } from "./ConfirmDialog";
import { NewChatButton } from "./NewChatButton";
import type { SessionSummary } from "../types/chat";

function formatTime(ts: number): string {
  const date = new Date(ts * 1000);
  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();
  if (isToday) {
    return date.toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
    });
  }
  return date.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
}

interface SessionItemProps {
  session: SessionSummary;
  isActive: boolean;
  onSelect: () => void;
  onRename: (title: string) => void;
  onDelete: () => void;
}

function SessionItem({
  session,
  isActive,
  onSelect,
  onRename,
  onDelete,
}: SessionItemProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [editTitle, setEditTitle] = useState(session.title);

  const handleRename = () => {
    const trimmed = editTitle.trim();
    if (trimmed && trimmed !== session.title) {
      onRename(trimmed);
    }
    setIsEditing(false);
  };

  return (
    <div
      onClick={onSelect}
      className={`
        group relative px-4 py-3 cursor-pointer border-l-3 transition-all
        ${
          isActive
            ? "bg-paper-dark border-moss"
            : "border-transparent hover:bg-paper-dark/50"
        }
      `}
    >
      {isEditing ? (
        <input
          autoFocus
          value={editTitle}
          onChange={(e) => setEditTitle(e.currentTarget.value)}
          onBlur={handleRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleRename();
            if (e.key === "Escape") setIsEditing(false);
          }}
          onClick={(e) => e.stopPropagation()}
          className="w-full text-sm bg-transparent border-b border-moss outline-none text-ink"
        />
      ) : (
        <>
          <div className="text-sm font-medium text-ink truncate pr-12">
            {session.title}
          </div>
          <div className="text-xs text-stone mt-1">{formatTime(session.updated_at)}</div>
          <div className="absolute right-2 top-2 hidden group-hover:flex gap-1">
            <button
              onClick={(e) => {
                e.stopPropagation();
                setIsEditing(true);
              }}
              className="p-2 text-xs text-stone hover:text-moss min-w-[28px] min-h-[28px] flex items-center justify-center"
              title="重命名"
            >
              ✎
            </button>
            <button
              onClick={(e) => {
                e.stopPropagation();
                onDelete();
              }}
              className="p-2 text-xs text-stone hover:text-red-600 min-w-[28px] min-h-[28px] flex items-center justify-center"
              title="删除"
            >
              ×
            </button>
          </div>
        </>
      )}
    </div>
  );
}

interface SidebarProps {
  onOpenSettings: () => void;
}

export function Sidebar({ onOpenSettings }: SidebarProps) {
  const { sessions, currentSessionId, loadSessions, selectSession, deleteSession, renameSession } =
    useChatStore();
  const [deletingSession, setDeletingSession] = useState<SessionSummary | null>(null);

  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

  return (
    <aside className="w-64 flex flex-col bg-paper-dark border-r border-mist flex-shrink-0">
      <div className="p-4 border-b border-mist">
        <NewChatButton />
      </div>
      <div className="flex-1 overflow-y-auto py-2">
        {sessions.map((session) => (
          <SessionItem
            key={session.id}
            session={session}
            isActive={session.id === currentSessionId}
            onSelect={() => selectSession(session.id)}
            onRename={(title) => renameSession(session.id, title)}
            onDelete={() => setDeletingSession(session)}
          />
        ))}
      </div>

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
        onConfirm={() => {
          if (deletingSession) {
            deleteSession(deletingSession.id);
          }
          setDeletingSession(null);
        }}
        onCancel={() => setDeletingSession(null)}
      />
    </aside>
  );
}
