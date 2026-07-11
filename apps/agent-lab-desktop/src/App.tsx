import { useState } from "react";
import { ChatHeader } from "./components/ChatHeader";
import { ChatInput } from "./components/ChatInput";
import { MessageList } from "./components/MessageList";
import { SettingsPanel } from "./components/SettingsPanel";
import { Sidebar } from "./components/Sidebar";

function App() {
  const [view, setView] = useState<"chat" | "settings">("chat");

  return (
    <main className="flex h-full bg-paper">
      <Sidebar onOpenSettings={() => setView("settings")} />
      {view === "settings" ? (
        <div className="flex-1 flex flex-col min-w-0">
          <header className="h-14 flex items-center justify-between px-6 border-b border-mist bg-paper">
            <h2 className="text-base font-medium text-ink">设置</h2>
            <button
              onClick={() => setView("chat")}
              className="px-3 py-1.5 text-xs text-stone hover:text-ink transition-colors"
            >
              返回聊天
            </button>
          </header>
          <SettingsPanel />
        </div>
      ) : (
        <div className="flex-1 flex flex-col min-w-0">
          <ChatHeader />
          <MessageList />
          <ChatInput />
        </div>
      )}
    </main>
  );
}

export default App;
