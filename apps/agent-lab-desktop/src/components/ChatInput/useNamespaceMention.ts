import { useCallback, useMemo, useState } from "react";
import { findTriggerInfo, getCaretOffset, getPlainText } from "./utils";

export interface UseNamespaceMentionOptions {
  editorRef: React.RefObject<HTMLDivElement | null>;
  namespaces: string[];
  onSelect: (namespace: string) => void;
}

export interface UseNamespaceMentionReturn {
  showMenu: boolean;
  menuIndex: number;
  menuAnchor: number;
  filteredNamespaces: string[];
  updateMenu: () => void;
  closeMenu: () => void;
  handleKeyDown: (event: React.KeyboardEvent<HTMLDivElement>) => boolean;
  selectNamespace: (namespace: string) => void;
}

export function useNamespaceMention({
  editorRef,
  namespaces,
  onSelect,
}: UseNamespaceMentionOptions): UseNamespaceMentionReturn {
  const [showMenu, setShowMenu] = useState(false);
  const [menuQuery, setMenuQuery] = useState("");
  const [menuIndex, setMenuIndex] = useState(0);
  const [menuAnchor, setMenuAnchor] = useState(0);

  const filteredNamespaces = useMemo(
    () =>
      namespaces
        .filter((ns) => ns.toLowerCase().includes(menuQuery.toLowerCase()))
        .slice(0, 8),
    [namespaces, menuQuery],
  );

  const updateMenu = useCallback(() => {
    const editor = editorRef.current;
    if (!editor) return;

    const text = getPlainText(editor);
    const cursor = getCaretOffset(editor);
    const info = findTriggerInfo(text, cursor, "$");

    if (!info) {
      setShowMenu(false);
      return;
    }

    setMenuAnchor(info.anchor);
    setMenuQuery(info.query);
    setMenuIndex(0);
    setShowMenu(true);
  }, [editorRef]);

  const closeMenu = useCallback(() => {
    setShowMenu(false);
  }, []);

  const selectNamespace = useCallback(
    (namespace: string) => {
      onSelect(namespace);
      setShowMenu(false);
    },
    [onSelect],
  );

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>): boolean => {
      if (!showMenu || filteredNamespaces.length === 0) return false;

      if (event.key === "ArrowDown") {
        event.preventDefault();
        setMenuIndex((i) => Math.min(i + 1, Math.max(filteredNamespaces.length - 1, 0)));
        return true;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setMenuIndex((i) => Math.max(i - 1, 0));
        return true;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        if (filteredNamespaces[menuIndex]) {
          selectNamespace(filteredNamespaces[menuIndex]);
        }
        return true;
      }
      if (event.key === "Escape") {
        setShowMenu(false);
        return true;
      }

      return false;
    },
    [showMenu, filteredNamespaces, menuIndex, selectNamespace],
  );

  return {
    showMenu,
    menuIndex,
    menuAnchor,
    filteredNamespaces,
    updateMenu,
    closeMenu,
    handleKeyDown,
    selectNamespace,
  };
}
