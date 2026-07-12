import type { ReactNode } from "react";
import type { ChatMessage } from "../../types/chat";
import { AssistantMessage } from "./AssistantMessage";
import { UserMessage } from "./UserMessage";

type MessageRenderer = (props: { message: ChatMessage }) => ReactNode;

const roleRenderers: Record<ChatMessage["role"], MessageRenderer | undefined> = {
  user: ({ message }) => <UserMessage message={message} />,
  assistant: ({ message }) => <AssistantMessage message={message} />,
  tool: undefined,
  system: undefined,
};

interface MessageItemProps {
  message: ChatMessage;
}

export function MessageItem({ message }: MessageItemProps) {
  const Renderer = roleRenderers[message.role];
  if (!Renderer) return null;
  return <Renderer message={message} />;
}

export { AssistantMessage } from "./AssistantMessage";
export { UserMessage } from "./UserMessage";
