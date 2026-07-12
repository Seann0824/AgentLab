import { useCallback, useMemo } from "react";
import { useChatStore } from "../../store/chatStore";
import type { ModelSelection } from "../../types/chat";

export interface ModelOption {
  key: string;
  label: string;
  providerId: string;
  model: string;
}

export interface ModelGroup {
  providerId: string;
  providerName: string;
  models: ModelOption[];
}

export interface UseModelSelectorReturn {
  modelOptions: ModelOption[];
  modelGroups: ModelGroup[];
  currentModelKey: string;
  selectModel: (key: string) => void;
  disabled: boolean;
}

export function useModelSelector(): UseModelSelectorReturn {
  const currentSessionId = useChatStore((s) => s.currentSessionId);
  const providers = useChatStore((s) => s.providers);
  // currentModelKey 只反映用户显式做的选择：
  // - 有真实 session 时，读取该 session 的 override
  // - 没有 session 时，读取虚拟 session 的 pendingSessionModel
  // 没有显式选择时显示为 ""（对应 UI 的"默认模型"）。
  const selectedModelOverride = useChatStore((s) =>
    currentSessionId
      ? (s.selectedModelBySession[currentSessionId] ?? null)
      : (s.pendingSessionModel ?? null),
  );
  const setSelectedModelForSession = useChatStore((s) => s.setSelectedModelForSession);
  const setPendingSessionModel = useChatStore((s) => s.setPendingSessionModel);
  const isStreaming = useChatStore((s) =>
    currentSessionId
      ? (s.streamingBySession[currentSessionId]?.isStreaming ?? false)
      : false,
  );

  const modelGroups = useMemo<ModelGroup[]>(
    () =>
      providers.map((p) => ({
        providerId: p.id,
        providerName: p.name,
        models: p.models.map((m) => ({
          key: `${p.id}::${m}`,
          label: m,
          providerId: p.id,
          model: m,
        })),
      })),
    [providers],
  );

  const modelOptions = useMemo<ModelOption[]>(
    () => modelGroups.flatMap((g) => g.models),
    [modelGroups],
  );

  const currentModelKey = useMemo(() => {
    if (!selectedModelOverride) return "";
    return `${selectedModelOverride.provider_id}::${selectedModelOverride.model}`;
  }, [selectedModelOverride]);

  const selectModel = useCallback(
    (key: string) => {
      if (!key) {
        if (currentSessionId) {
          setSelectedModelForSession(currentSessionId, null);
        } else {
          setPendingSessionModel(null);
        }
        return;
      }
      const option = modelOptions.find((o) => o.key === key);
      if (!option) return;
      const selection: ModelSelection = {
        provider_id: option.providerId,
        model: option.model,
      };
      if (currentSessionId) {
        setSelectedModelForSession(currentSessionId, selection);
      } else {
        setPendingSessionModel(selection);
      }
    },
    [currentSessionId, modelOptions, setSelectedModelForSession, setPendingSessionModel],
  );

  return {
    modelOptions,
    modelGroups,
    currentModelKey,
    selectModel,
    disabled: isStreaming || modelOptions.length === 0,
  };
}
