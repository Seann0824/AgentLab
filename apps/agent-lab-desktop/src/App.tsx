import { useState } from "react";
import reactLogo from "./assets/react.svg";
import { invoke, Channel } from "@tauri-apps/api/core";
import { commands } from "./bindings";
import "./App.css";

interface FileChunk {
  chunk: number[];
  progress: number;
  done: boolean;
}

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
  const [greetMsg, setGreetMsg] = useState("");
  const [name, setName] = useState("");
  const [chatInput, setChatInput] = useState("");
  const [chatOutput, setChatOutput] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);

  async function greet() {
    // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
    setGreetMsg(await commands.greet(name));
  }

  async function getGitignoreFile() {
    const result = await commands.readFile(
      "/Users/sean/Desktop/repo/agent-lab/.gitignore",
    );

    if (result.status === "error") {
      console.error("read file failed:", result.error);
      return;
    }

    // 返回 number[]，转成 Uint8Array 再解码
    const data = new Uint8Array(result.data);
    console.log("file size:", data.length);
    console.log("file content:", new TextDecoder().decode(data));
  }

  async function getGitignoreFileByChannel() {
    const chunks: Uint8Array[] = [];
    const channel = new Channel<FileChunk>((payload) => {
      console.log(
        "progress:",
        (payload.progress * 100).toFixed(2) + "%",
        "done:",
        payload.done,
      );
      if (payload.chunk.length > 0) {
        chunks.push(new Uint8Array(payload.chunk));
      }
    });

    await invoke("read_file_channel", {
      filePath: "/Users/sean/Desktop/repo/agent-lab/.gitignore",
      onChunk: channel,
    });

    const totalLength = chunks.reduce((sum, c) => sum + c.length, 0);
    const result = new Uint8Array(totalLength);
    let offset = 0;
    for (const chunk of chunks) {
      result.set(chunk, offset);
      offset += chunk.length;
    }

    console.log("file content:", new TextDecoder().decode(result));
  }

  async function login() {
    const result = await commands.login("tauri", "tauri");
    console.log("login", result);
  }

  async function chat() {
    if (!chatInput.trim()) return;

    setChatOutput("");
    let fullContent = "";

    const channel = new Channel<AgentStreamEvent>((event) => {
      console.log("AgentStreamEvent:", event);
      console.log("AgentStreamEvent Type", event.type);

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
      <h1>Welcome to Tauri + React</h1>

      <div className="row">
        <a href="https://vite.dev" target="_blank">
          <img src="/vite.svg" className="logo vite" alt="Vite logo" />
        </a>
        <a href="https://tauri.app" target="_blank">
          <img src="/tauri.svg" className="logo tauri" alt="Tauri logo" />
        </a>
        <a href="https://react.dev" target="_blank">
          <img src={reactLogo} className="logo react" alt="React logo" />
        </a>
      </div>
      <p>Click on the Tauri, Vite, and React logos to learn more.</p>
      <button onClick={() => login()}>登录</button>
      <button onClick={() => getGitignoreFileByChannel()}>
        通过 Channel 读取文件
      </button>
      <form
        className="row"
        onSubmit={async (e) => {
          e.preventDefault();
          greet();
          getGitignoreFile();
        }}
      >
        <input
          id="greet-input"
          onChange={(e) => setName(e.currentTarget.value)}
          placeholder="Enter a name..."
        />
        <button type="submit">Greet</button>
      </form>
      <p>{greetMsg}</p>

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
