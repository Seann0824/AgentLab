import { useCallback, useMemo } from "react";
import { useChatStore } from "../../store/chatStore";
import type { ModelSelection } from "../../types/chat";

export interface ModelOption {
  key: string;
  label: string;
  providerId: string;
  model: string;
}

export interface UseModelSelectorReturn {
  modelOptions: ModelOption[];
  currentModelKey: string;
  handleModelChange: (event: React.ChangeEvent<HTMLSelectElement>) => void;
  disabled: boolean;
}

export function useModelSelector(): UseModelSelectorReturn {
  const currentSessionId = useChatStore((s) => s.currentSessionId);
  const providers = useChatStore((s) => s.providers);
  const defaultModel = useChatStore((s) => s.defaultModel);
  const selectedModel = useChatStore((s) =>
    currentSessionId ? s.getSelectedModelForSession(currentSessionId) : s.defaultModel,
  );
  const setSelectedModelForSession = useChatStore((s) => s.setSelectedModelForSession);
  const isStreaming = useChatStore((s) =>
    currentSessionId
      ? (s.streamingBySession[currentSessionId]?.isStreaming ?? false)
      : false,
  );

  const modelOptions = useMemo<ModelOption[]>(
    () =>
      providers.flatMap((p) =>
        p.models.map((m) => ({
          key: `${p.id}::${m}`,
          label: `${p.name} / ${m}`,
          providerId: p.id,
          model: m,
        })),
      ),
    [providers],
  );

  const currentModelKey = useMemo(() => {
    const target = selectedModel ?? defaultModel;
    return target ? `${target.provider_id}::${target.model}` : "";
  }, [selectedModel, defaultModel]);

  const handleModelChange = useCallback(
    (event: React.ChangeEvent<HTMLSelectElement>) => {
      const value = event.currentTarget.value;
      if (!currentSessionId) return;
      if (!value) {
        setSelectedModelForSession(currentSessionId, null);
        return;
      }
      const option = modelOptions.find((o) => o.key === value);
      if (option) {
        const selection: ModelSelection = {
          provider_id: option.providerId,
          model: option.model,
        };
        setSelectedModelForSession(currentSessionId, selection);
      }
    },
    [currentSessionId, modelOptions, setSelectedModelForSession],
  );

  return {
    modelOptions,
    currentModelKey,
    handleModelChange,
    disabled: isStreaming || modelOptions.length === 0,
  };
}
