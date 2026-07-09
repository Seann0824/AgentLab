import { useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import "./App.css";

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

  return (
    <main className="container">
      <h1>Agent Lab Desktop</h1>

      <div className="row" style={{ marginTop: 24 }}>
        <input
          value={chatInput}
          onChange={(e) => setChatInput(e.currentTarget.value)}
          placeholder="输入消息..."
          style={{ minWidth: 240 }}
        />
        <button onClick={chat}>发送</button>
      </div>
      <p>sessionId: {sessionId ?? "新会话"}</p>
      <pre
        style={{
          whiteSpace: "pre-wrap",
          textAlign: "left",
          maxWidth: 600,
          border: "1px solid #ccc",
          padding: 12,
        }}
      >
        {chatOutput || "等待回复..."}
      </pre>
    </main>
  );
}

export default App;
