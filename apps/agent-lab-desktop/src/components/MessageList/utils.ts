import type { ChatMessage } from "../../types/chat";

export interface MessageGroup {
  type: "user" | "assistant";
  message: ChatMessage;
  toolMessages: ChatMessage[];
}

export function groupMessages(messages: ChatMessage[]): MessageGroup[] {
  const groups: MessageGroup[] = [];
  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    if (msg.role === "user") {
      groups.push({ type: "user", message: msg, toolMessages: [] });
    } else if (msg.role === "assistant") {
      const toolMessages: ChatMessage[] = [];
      let j = i + 1;
      while (j < messages.length && messages[j].role === "tool") {
        toolMessages.push(messages[j]);
        j++;
      }
      groups.push({ type: "assistant", message: msg, toolMessages });
      i = j - 1;
    }
  }
  return groups;
}
