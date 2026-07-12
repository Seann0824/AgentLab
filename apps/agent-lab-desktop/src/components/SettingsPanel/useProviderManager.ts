import { useCallback, useState } from "react";
import { useChatStore } from "../../store/chatStore";
import type { ProviderConfig } from "../../types/chat";
import { emptyProvider } from "./utils";

export interface UseProviderManagerReturn {
  providers: ProviderConfig[];
  editingProvider: ProviderConfig | null;
  deletingProvider: ProviderConfig | null;
  providerMessage: string;
  startCreate: () => void;
  startEdit: (provider: ProviderConfig) => void;
  cancelEdit: () => void;
  updateField: <K extends keyof ProviderConfig>(
    field: K,
    value: ProviderConfig[K],
  ) => void;
  updateModel: (index: number, value: string) => void;
  addModel: () => void;
  removeModel: (index: number) => void;
  handleSave: () => Promise<void>;
  requestDelete: (provider: ProviderConfig) => void;
  cancelDelete: () => void;
  confirmDelete: () => Promise<void>;
}

export function useProviderManager(): UseProviderManagerReturn {
  const providers = useChatStore((s) => s.providers);
  const createOrUpdateProvider = useChatStore((s) => s.createOrUpdateProvider);
  const removeProvider = useChatStore((s) => s.removeProvider);

  const [editingProvider, setEditingProvider] = useState<ProviderConfig | null>(null);
  const [deletingProvider, setDeletingProvider] = useState<ProviderConfig | null>(null);
  const [providerMessage, setProviderMessage] = useState("");

  const startCreate = useCallback(() => {
    setEditingProvider(emptyProvider());
    setProviderMessage("");
  }, []);

  const startEdit = useCallback((provider: ProviderConfig) => {
    setEditingProvider(provider);
    setProviderMessage("");
  }, []);

  const cancelEdit = useCallback(() => {
    setEditingProvider(null);
  }, []);

  const updateField = useCallback(<K extends keyof ProviderConfig>(
    field: K,
    value: ProviderConfig[K],
  ) => {
    setEditingProvider((prev) => (prev ? { ...prev, [field]: value } : prev));
  }, []);

  const updateModel = useCallback((index: number, value: string) => {
    setEditingProvider((prev) => {
      if (!prev) return prev;
      const models = [...prev.models];
      models[index] = value;
      return { ...prev, models };
    });
  }, []);

  const addModel = useCallback(() => {
    setEditingProvider((prev) =>
      prev ? { ...prev, models: [...prev.models, ""] } : prev,
    );
  }, []);

  const removeModel = useCallback((index: number) => {
    setEditingProvider((prev) => {
      if (!prev) return prev;
      return { ...prev, models: prev.models.filter((_, i) => i !== index) };
    });
  }, []);

  const handleSave = useCallback(async () => {
    if (!editingProvider) return;
    const p = editingProvider;
    if (!p.name.trim() || !p.base_url.trim()) {
      setProviderMessage("请填写名称和 Base URL");
      return;
    }
    if (p.models.length === 0 || p.models.some((m) => !m.trim())) {
      setProviderMessage("请至少填写一个有效模型");
      return;
    }

    try {
      await createOrUpdateProvider(p);
      setEditingProvider(null);
      setProviderMessage("已保存");
    } catch (err) {
      setProviderMessage(`保存失败：${err}`);
    }
  }, [editingProvider, createOrUpdateProvider]);

  const requestDelete = useCallback((provider: ProviderConfig) => {
    setDeletingProvider(provider);
  }, []);

  const cancelDelete = useCallback(() => {
    setDeletingProvider(null);
  }, []);

  const confirmDelete = useCallback(async () => {
    const p = deletingProvider;
    if (!p) return;
    setDeletingProvider(null);
    try {
      await removeProvider(p.id);
      setProviderMessage(`已删除 ${p.name}`);
    } catch (err) {
      setProviderMessage(`删除失败：${err}`);
    }
  }, [deletingProvider, removeProvider]);

  return {
    providers,
    editingProvider,
    deletingProvider,
    providerMessage,
    startCreate,
    startEdit,
    cancelEdit,
    updateField,
    updateModel,
    addModel,
    removeModel,
    handleSave,
    requestDelete,
    cancelDelete,
    confirmDelete,
  };
}
