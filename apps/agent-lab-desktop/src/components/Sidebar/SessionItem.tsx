import type { SessionSummary } from "../../types/chat";
import { useInlineEdit } from "./useInlineEdit";
import { formatTime } from "./utils";

interface SessionItemProps {
  session: SessionSummary;
  isActive: boolean;
  isStreaming: boolean;
  unreadCount: number;
  collapsed?: boolean;
  onSelect: () => void;
  onRename: (title: string) => void;
  onDelete: () => void;
}

export function SessionItem({
  session,
  isActive,
  isStreaming,
  unreadCount,
  collapsed,
  onSelect,
  onRename,
  onDelete,
}: SessionItemProps) {
  const edit = useInlineEdit(session.title, onRename);

  if (collapsed) {
    return (
      <div
        onClick={onSelect}
        title={session.title}
        className={`
          group relative py-3 px-2 cursor-pointer transition-all flex justify-center
          ${
            isActive
              ? "bg-paper-dark"
              : "hover:bg-paper-dark/50"
          }
        `}
      >
        <div
          className={`
            w-9 h-9 rounded-full flex items-center justify-center text-xs font-medium
            ${isActive ? "bg-moss text-paper" : "bg-paper text-ink"}
          `}
        >
          {session.title.charAt(0) || "?"}
        </div>
        <div className="absolute right-1 top-1 flex flex-col items-center gap-1">
          {isStreaming && (
            <span
              className="w-1.5 h-1.5 rounded-full bg-moss animate-pulse"
              title="思考中…"
            />
          )}
          {!isStreaming && unreadCount > 0 && (
            <span className="min-w-[14px] h-[14px] px-0.5 flex items-center justify-center rounded-full bg-red-500 text-white text-[9px] font-medium">
              {unreadCount > 99 ? "99+" : unreadCount}
            </span>
          )}
        </div>
      </div>
    );
  }

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
      {edit.isEditing ? (
        <input
          autoFocus
          value={edit.editTitle}
          onChange={(e) => edit.setEditTitle(e.currentTarget.value)}
          onBlur={edit.handleBlur}
          onKeyDown={edit.handleKeyDown}
          onClick={(e) => e.stopPropagation()}
          className="w-full text-sm bg-transparent border-b border-moss outline-none text-ink"
        />
      ) : (
        <>
          <div className="text-sm font-medium text-ink truncate pr-12">
            {session.title}
          </div>
          <div className="text-xs text-stone mt-1">{formatTime(session.updated_at)}</div>
          <div className="absolute right-2 top-2 flex items-center gap-1">
            <div className="hidden group-hover:flex gap-1">
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  edit.startEdit();
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
            <div className="flex items-center justify-center min-w-[28px] min-h-[28px] group-hover:hidden">
              {isStreaming && (
                <span
                  className="w-2 h-2 rounded-full bg-moss animate-pulse"
                  title="思考中…"
                />
              )}
              {!isStreaming && unreadCount > 0 && (
                <span className="min-w-[18px] h-[18px] px-1 flex items-center justify-center rounded-full bg-red-500 text-white text-[10px] font-medium">
                  {unreadCount > 99 ? "99+" : unreadCount}
                </span>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
