import { useCallback, useState } from "react";

export interface UseInlineEditReturn {
  isEditing: boolean;
  editTitle: string;
  setEditTitle: (value: string) => void;
  startEdit: () => void;
  handleRename: () => void;
  handleKeyDown: (event: React.KeyboardEvent<HTMLInputElement>) => void;
  handleBlur: () => void;
}

export function useInlineEdit(
  title: string,
  onRename: (title: string) => void,
): UseInlineEditReturn {
  const [isEditing, setIsEditing] = useState(false);
  const [editTitle, setEditTitle] = useState(title);

  const startEdit = useCallback(() => {
    setEditTitle(title);
    setIsEditing(true);
  }, [title]);

  const handleRename = useCallback(() => {
    const trimmed = editTitle.trim();
    if (trimmed && trimmed !== title) {
      onRename(trimmed);
    }
    setIsEditing(false);
  }, [editTitle, title, onRename]);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "Enter") {
        handleRename();
      } else if (event.key === "Escape") {
        setIsEditing(false);
      }
    },
    [handleRename],
  );

  const handleBlur = useCallback(() => {
    handleRename();
  }, [handleRename]);

  return {
    isEditing,
    editTitle,
    setEditTitle,
    startEdit,
    handleRename,
    handleKeyDown,
    handleBlur,
  };
}
