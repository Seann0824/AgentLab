import { useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";

type AgentStreamEvent =
  | { type: "content"; delta: string }
  | { type: "reason"; delta: string }
  | { type: "content_done"; content: string }
  | { type: "reason_done"; reason: string }
  | { type: "tool_call"; tool_name: string; tool_call_id: string }
  | {
      type: "tool_call_result";
      is_error: boolean;
      tool_name: string;
      tool_call_id: string;
      tool_call_result: string;
    };

function App() {
  const [chatInput, setChatInput] = useState("");
  const [chatOutput, setChatOutput] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);

  async function chat() {
    if (!chatInput.trim()) return;

    setChatOutput("");
    let fullContent = "";

    const channel = new Channel<AgentStreamEvent>((event) => {
      switch (event.type) {
        case "content":
          fullContent += event.delta;
          setChatOutput(fullContent);
          break;
        case "content_done":
          fullContent = event.content;
          setChatOutput(fullContent);
          break;
        case "tool_call":
          console.log("调用工具:", event.tool_name);
          break;
        case "tool_call_result":
          console.log(`工具结果 [${event.tool_name}]:`, event.tool_call_result);
          break;
      }
    });

    const returnedSessionId = await invoke<string>("chat_completion_stream", {
      sessionId,
      message: chatInput,
      channel,
    });

    setSessionId(returnedSessionId);
    console.log("sessionId:", returnedSessionId);
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      chat();
    }
  };

  return (
    <main className="min-h-full flex flex-col items-center justify-center px-6 py-16 bg-paper">
      <div className="w-full max-w-2xl">
        <header className="mb-16 text-center">
          <h1 className="text-4xl font-light tracking-wider text-ink mb-4">
            Agent Lab
          </h1>
          <p className="text-stone text-sm tracking-wide">
            与日本简约之美同行的智能助手
          </p>
        </header>

        <section className="card-paper p-8 mb-8">
          <div className="flex items-stretch gap-3">
            <input
              className="input-minimal flex-1"
              value={chatInput}
              onChange={(e) => setChatInput(e.currentTarget.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入消息，按 Enter 发送..."
            />
            <button className="btn-moss" onClick={chat}>
              发送
            </button>
          </div>
        </section>

        <section className="card-paper p-8 min-h-[240px]">
          <div className="flex items-center justify-between mb-6 pb-4 border-b border-mist">
            <span className="text-xs uppercase tracking-wider text-stone">
              对话
            </span>
            <span className="text-xs text-stone-light">
              {sessionId ?? "新会话"}
            </span>
          </div>

          <pre className="whitespace-pre-wrap font-sans text-ink-light leading-relaxed">
            {chatOutput || (
              <span className="text-stone-light">等待回复…</span>
            )}
          </pre>
        </section>
      </div>
    </main>
  );
}

export default App;
