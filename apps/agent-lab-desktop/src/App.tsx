import { ChatHeader } from "./components/ChatHeader";
import { ChatInput } from "./components/ChatInput";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";

function App() {
  return (
    <main className="flex h-full bg-paper">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <ChatHeader />
        <MessageList />
        <ChatInput />
      </div>
    </main>
  );
}

export default App;
