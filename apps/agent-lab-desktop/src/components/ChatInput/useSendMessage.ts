import { useCallback } from "react";
import { useChatStore } from "../../store/chatStore";

export interface UseSendMessageReturn {
  isStreaming: boolean;
  handleSend: (text: string) => Promise<void>;
}

export function useSendMessage(): UseSendMessageReturn {
  const currentSessionId = useChatStore((s) => s.currentSessionId);
  const isStreaming = useChatStore((s) =>
    currentSessionId
      ? (s.streamingBySession[currentSessionId]?.isStreaming ?? false)
      : false,
  );
  const sendMessage = useChatStore((s) => s.sendMessage);

  const handleSend = useCallback(
    async (text: string) => {
      if (!currentSessionId || isStreaming) return;
      const trimmed = text.trim();
      if (!trimmed) return;
      await sendMessage(trimmed);
    },
    [currentSessionId, isStreaming, sendMessage],
  );

  return { isStreaming, handleSend };
}
