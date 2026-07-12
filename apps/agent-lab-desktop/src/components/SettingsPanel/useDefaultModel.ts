import { useCallback, useMemo, useState } from "react";
import { useChatStore } from "../../store/chatStore";
import type { ModelSelection } from "../../types/chat";

export interface DefaultModelOption {
  providerId: string;
  providerName: string;
  model: string;
}

export interface UseDefaultModelReturn {
  defaultModel: ModelSelection | null;
  options: DefaultModelOption[];
  message: string;
  handleSetDefaultModel: (selection: ModelSelection) => Promise<void>;
}

export function useDefaultModel(): UseDefaultModelReturn {
  const defaultModel = useChatStore((s) => s.defaultModel);
  const providers = useChatStore((s) => s.providers);
  const setDefaultModel = useChatStore((s) => s.setDefaultModel);
  const [message, setMessage] = useState("");

  const options = useMemo<DefaultModelOption[]>(
    () =>
      providers.flatMap((p) =>
        p.models.map((m) => ({
          providerId: p.id,
          providerName: p.name,
          model: m,
        })),
      ),
    [providers],
  );

  const handleSetDefaultModel = useCallback(
    async (selection: ModelSelection) => {
      try {
        await setDefaultModel(selection);
        setMessage("默认模型已更新");
      } catch (err) {
        setMessage(`更新默认模型失败：${err}`);
      }
    },
    [setDefaultModel],
  );

  return {
    defaultModel,
    options,
    message,
    handleSetDefaultModel,
  };
}
