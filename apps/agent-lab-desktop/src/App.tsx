import { useState } from "react";
import reactLogo from "./assets/react.svg";
import { invoke, Channel } from "@tauri-apps/api/core";
import "./App.css";

interface FileChunk {
  chunk: number[];
  progress: number;
  done: boolean;
}

function App() {
  const [greetMsg, setGreetMsg] = useState("");
  const [name, setName] = useState("");

  async function greet() {
    // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
    setGreetMsg(await invoke("greet", { name }));
  }

  async function getGitignoreFile() {
    try {
      let file = await invoke("read_file", {
        filePath: "/Users/sean/Desktop/repo/agent-lab/.gitignore",
      });

      // file 是 Uint8Array / ArrayBuffer
      console.log("file size:", (file as Uint8Array).length);
      console.log(
        "file content:",
        new TextDecoder().decode(file as Uint8Array)
      );
    } catch (error) {
      console.error("read file failed:", error);
    }
  }

  async function getGitignoreFileByChannel() {
    const chunks: Uint8Array[] = [];
    const channel = new Channel<FileChunk>((payload) => {
      console.log(
        "progress:",
        (payload.progress * 100).toFixed(2) + "%",
        "done:",
        payload.done
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

  function login() {
    const payload = {
      user: "tauri",
      password: "tauri",
    };
    console.log("login");
    invoke("login", payload)
      .then((message) => console.log(message))
      .catch((error) => console.error(error));
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
    </main>
  );
}

export default App;
