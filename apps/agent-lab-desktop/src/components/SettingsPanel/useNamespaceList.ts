import { useCallback, useState } from "react";
import { useChatStore } from "../../store/chatStore";

export interface UseNamespaceListReturn {
  namespaces: string[];
  deletingNamespace: string | null;
  message: string;
  requestDelete: (namespace: string) => void;
  cancelDelete: () => void;
  confirmDelete: () => Promise<void>;
}

export function useNamespaceList(): UseNamespaceListReturn {
  const namespaces = useChatStore((s) => s.namespaces);
  const deleteNamespace = useChatStore((s) => s.deleteNamespace);

  const [deletingNamespace, setDeletingNamespace] = useState<string | null>(null);
  const [message, setMessage] = useState("");

  const requestDelete = useCallback((namespace: string) => {
    setDeletingNamespace(namespace);
  }, []);

  const cancelDelete = useCallback(() => {
    setDeletingNamespace(null);
  }, []);

  const confirmDelete = useCallback(async () => {
    const ns = deletingNamespace;
    if (!ns) return;
    setDeletingNamespace(null);
    try {
      await deleteNamespace(ns);
      setMessage(`已删除 ${ns}`);
    } catch (err) {
      setMessage(`删除失败：${err}`);
    }
  }, [deletingNamespace, deleteNamespace]);

  return {
    namespaces,
    deletingNamespace,
    message,
    requestDelete,
    cancelDelete,
    confirmDelete,
  };
}
