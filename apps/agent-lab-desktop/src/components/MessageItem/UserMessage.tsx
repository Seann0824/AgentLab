import { memo } from "react";
import type { ChatMessage } from "../../types/chat";

export const UserMessage = memo(function UserMessage({
  message,
}: {
  message: ChatMessage;
}) {
  return (
    <div className="flex justify-end mb-6">
      <div className="max-w-[80%] px-5 py-3 bg-white rounded-2xl rounded-tr-sm text-ink text-sm leading-relaxed shadow-sm">
        {message.content}
      </div>
    </div>
  );
});
