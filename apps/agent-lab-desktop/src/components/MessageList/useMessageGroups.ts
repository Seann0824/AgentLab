import { useMemo } from "react";
import type { ChatMessage } from "../../types/chat";
import { groupMessages, type MessageGroup } from "./utils";

export function useMessageGroups(messages: ChatMessage[]): MessageGroup[] {
  return useMemo(() => groupMessages(messages), [messages]);
}
