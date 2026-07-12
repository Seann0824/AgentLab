import { useChatStore } from "../store/chatStore";

export function NewChatButton() {
  const startNewChat = useChatStore((s) => s.startNewChat);

  return (
    <button
      onClick={startNewChat}
      className="w-full flex items-center justify-center gap-2 px-4 py-2.5 text-sm font-medium text-paper bg-moss rounded-sm hover:bg-moss/90 transition-colors"
    >
      <span className="text-lg leading-none">+</span>
      <span>新会话</span>
    </button>
  );
}
