# agent-lab 项目架构文档

> **版本**: 0.1.0  
> **语言**: Rust (Edition 2024)  
> **项目定位**: 一个基于大语言模型的终端 Agent 框架，充当 AI 与外部世界的桥梁。

---

## 1. 项目概述

`agent-lab` 是一个运行在终端中的 AI Agent 应用。它通过 **ReAct 模式（推理-行动循环）** 连接大语言模型和本地工具（如 Shell 命令），使 AI 能够理解用户意图、调用工具完成任务，并将结果反馈给用户。

核心流程为：

```
用户输入 → AI 模型推理 → [工具调用] → 工具执行 → 结果回传 → AI 继续推理 → 输出最终回复
```

---

## 2. 整体架构图

```
┌─────────────────────────────────────────────────────────────────────┐
│                         main.rs (入口)                              │
│                                                                     │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────┐  │
│  │ initial_model │    │initial_tool  │    │   主交互循环          │  │
│  │ ()            │    │_manager()    │    │ loop { 输入→推理→工具 }│  │
│  └──────┬───────┘    └──────┬───────┘    └──────────┬───────────┘  │
│         │                   │                        │              │
└─────────┼───────────────────┼────────────────────────┼──────────────┘
          │                   │                        │
          ▼                   ▼                        ▼
   ┌─────────────┐    ┌─────────────┐        ┌──────────────┐
   │  Model 层    │    │  Tools 层    │        │ ChatMessage  │
   │ (AI 适配器)  │    │ (工具管理)   │        │ 消息流转      │
   └─────────────┘    └─────────────┘        └──────────────┘
```

---

## 3. 模块详解

### 3.1 入口层 — `src/main.rs`

| 函数 | 职责 |
|------|------|
| `main()` | 应用入口，初始化模型和工具管理器，进入主交互循环 |
| `initial_model()` | 从环境变量读取配置，初始化 AI 模型适配器（目前为 DeepSeek） |
| `initial_tool_manager()` | 创建工具管理器并注册内置工具（如 BashShell） |
| `finish_terminal_line()` | 终端输出格式辅助函数 |

**主循环逻辑流程：**

```
┌──────────┐     ┌──────────┐     ┌──────────────┐     ┌──────────┐
│ 用户输入  │────▶│ 模型流式 │────▶│ 工具并行执行  │────▶│ 结果回传 │
│ (stdin)  │     │ 推理     │     │ (Futures-    │     │ 给模型   │
└──────────┘     │ (SSE)   │     │  Unordered)  │     └──────────┘
                 └──────────┘     └──────────────┘
                        │                                  │
                        ▼                                  ▼
                  ┌────────────┐                   ┌──────────────┐
                  │ 自动模式   │◀───────────────────│ 有工具调用时  │
                  │ (is_auto)  │                   └──────────────┘
                  └────────────┘
```

- **自动模式**：当模型发起工具调用时，`is_auto` 设为 `true`，跳过用户输入，自动将工具结果回传给模型继续推理。
- **手动模式**：当模型只输出文本（无工具调用），`is_auto` 设为 `false`，等待用户输入新指令。

---

### 3.2 模型层 — `src/model/`

位于 `src/model/` 目录下。

#### 3.2.1 核心类型 — `types.rs`

| 类型 | 说明 |
|------|------|
| `ToolCall` | 工具调用结构体，包含 `id`、`name`、`arguments`（JSON 字符串） |
| `ChatMessage` | 聊天消息枚举，支持 `System`、`User`、`Assistant`、`Tool` 四种角色 |
| `ModelEvent` | 模型流式事件枚举，包含 `Text`、`Thinking`、`ToolCallBlock`、`Done`、`Error` |
| `ModelAdapter` | 模型适配器 trait，定义 `stream_chat()` 接口 |
| `ModelStream` | 流式响应类型别名 `Pin<Box<dyn Stream<Item = ModelEvent>>>` |

**ChatMessage 角色：**

```
System  ──▶ 系统提示词（固定设定）
User    ──▶ 用户输入
Assistant ─▶ 模型回复（含可选的 tool_calls）
Tool    ──▶ 工具执行结果
```

#### 3.2.2 OpenAI 兼容适配器 — `openai_compatible.rs`

实现 `ModelAdapter` trait，对接 **兼容 OpenAI API 格式** 的大模型服务。

- 支持 **SSE（Server-Sent Events）流式输出**
- 使用 `mpsc` channel + `ReceiverStream` 构建异步流
- 处理 `reasoning_content`（思维链内容，显示为灰色）
- 处理 `tool_calls`（增量式工具调用参数拼接）
- 支持 `finish_reason: "tool_calls"` 判断工具调用结束

**当前配置（通过 .env 读取）：**

```
DEEPSEEK_BASE_URL = https://api.deepseek.com
DEEPSEEK_API_KEY  = sk-...
MODEL             = deepseek-v4-flash
```

> **设计意图**：OpenAI 兼容格式是目前最通用的 AI API 标准，通过此适配器可对接 DeepSeek、OpenAI、Claude（Anthropic 兼容模式）等大多数主流模型服务。

---

### 3.3 工具层 — `src/tools/`

位于 `src/tools/` 目录下，采用 **插件化架构**，方便扩展新工具。

#### 3.3.1 工具核心定义 — `types.rs`

| 类型 | 说明 |
|------|------|
| `Tool` trait | 工具接口，定义 `name()`、`description()`、`parameters_schema()`、`execute()` |
| `ToolEvent` | 工具执行事件枚举：`Progress`、`Done(Value)`、`Err(String)` |
| `ToolStream` | 工具执行流类型别名 `Pin<Box<dyn Stream<Item = ToolEvent>>>` |

**Tool trait 定义：**

```rust
pub trait Tool {
    fn name(&self) -> &str;               // 工具名称
    fn description(&self) -> &str;        // 工具描述
    fn parameters_schema(&self) -> Value; // JSON Schema 参数定义
    fn execute(&self, args: Value) -> ToolStream; // 执行逻辑
}
```

#### 3.3.2 工具管理器 — `mod.rs`

`ToolManager` 负责工具的注册、Schema 收集和调度执行：

| 方法 | 职责 |
|------|------|
| `new()` | 创建管理器 |
| `register_tool()` | 注册工具（名称 → 工具实例映射） |
| `get_tools_schema()` | 收集所有工具的 JSON Schema，组装后传给模型 |
| `run()` | 根据 `ToolCall` 查找并执行对应工具，返回 `ChatMessage::Tool` |

**工具调度流程：**

```
ToolCall { id, name, arguments }
    │
    ▼
┌──────────────┐   No   ┌─────────────────────┐
│ 工具是否存在？ │──────▶│ 返回 unknown_tool 错误│
└──────┬───────┘       └─────────────────────┘
       │ Yes
       ▼
┌──────────────┐   No   ┌─────────────────────────┐
│ 参数 JSON 有效 │──────▶│ 返回 invalid_arguments  │
└──────┬───────┘       └─────────────────────────┘
       │ Yes
       ▼
  执行工具 execute() → 等待 ToolEvent::Done / Err
       │
       ▼
┌──────────────────┐
│ 返回 ChatMessage │
│ ::Tool(result)   │
└──────────────────┘
```

#### 3.3.3 内置工具: BashShell — `base_shell/mod.rs`

| 属性 | 值 |
|------|-----|
| 名称 | `shell` |
| 描述 | 在本地执行 CLI 命令 |
| 参数 | `{ "command": "string" }` |
| 超时 | 30 分钟（`30 * 60 * 1000` 毫秒） |
| Shell | 使用 `zsh -lc` 执行 |

**返回结果格式：**

```json
{
  "command": "ls -la",
  "status": 0,
  "success": true,
  "stdout": "...",
  "stderr": ""
}
```

---

## 4. 数据流全景

### 4.1 一次完整交互的时序

```
User                  main()                 Model Adapter              Tool Manager         BashShell
  │                     │                        │                        │                    │
  │── stdin输入 ───────▶│                        │                        │                    │
  │                     │── stream_chat() ──────▶│                        │                    │
  │                     │                        │── (SSE 流式推理)       │                    │
  │                     │◀── ModelEvent::Text ───│                        │                    │
  │                     │◀── ModelEvent::Thinking │                        │                    │
  │                     │◀── ModelEvent::ToolCallBlock ──│                │                    │
  │                     │                        │                        │                    │
  │                     │── ToolManager.run() ──────────────────────────▶│                    │
  │                     │                        │                        │── execute() ──────▶│
  │                     │                        │                        │◀── ToolEvent::Done │
  │                     │◀── ChatMessage::Tool ──│                        │                    │
  │                     │                        │                        │                    │
  │                     │── stream_chat() (自动)─▶│                       │                    │
  │                     │◀── ModelEvent::Text ───│                        │                    │
  │                     │◀── ModelEvent::Done ───│                        │                    │
  │── stdout输出 ──────▶│                        │                        │                    │
```

### 4.2 消息格式转换

```
内部 ChatMessage ──▶ OpenAI 消息格式 ──▶ HTTP POST ──▶ SSE 流解析 ──▶ ModelEvent
                                                                          │
      ◀────────────────────────────────────────────────────────────────────┘
```

---

## 5. 设计模式与架构特点

### 5.1 关键设计模式

| 模式 | 应用位置 | 说明 |
|------|---------|------|
| **Adapter 模式** | `model/` 层 | `ModelAdapter` trait 统一模型接口，`OpenAiCompatibleAdapter` 是对 OpenAI API 的具体适配 |
| **Strategy 模式** | `tools/` 层 | `Tool` trait 定义统一策略接口，各工具独立实现 |
| **Observer + Stream 模式** | 模型流/工具流 | 使用 `mpsc channel` + `Pin<Box<dyn Stream>>` 实现异步事件流 |
| **Plugin 模式** | `ToolManager` | 工具通过 `register_tool()` 动态注册 |

### 5.2 架构特性

- **异步全栈**：基于 `tokio` 运行时，所有 I/O 操作（网络请求、子进程执行）均为异步
- **流式响应**：模型的思维过程（Thinking）和最终回复（Text）通过事件流实时推送
- **自动推理链**：支持模型多轮工具调用，无需用户干预（`is_auto` 标志控制）
- **可扩展性**：新增工具只需实现 `Tool` trait 并注册即可

---

## 6. 目录结构与文件职责

```
agent-lab/
├── Cargo.toml              # 项目配置与依赖清单
├── .env                    # 环境变量（API Key、Base URL）
├── .gitignore
├── docs/
│   └── architecture.md     # 本文档
├── src/
│   ├── main.rs             # 程序入口，交互循环
│   ├── agent.rs            # 预留，未来可能的 Agent 抽象
│   ├── model/
│   │   ├── mod.rs          # 模块导出
│   │   ├── types.rs        # 核心类型：ChatMessage, ModelEvent, ModelAdapter
│   │   └── openai_compatible.rs  # OpenAI 兼容 API 适配器
│   └── tools/
│       ├── mod.rs          # ToolManager 工具管理器
│       ├── types.rs        # Tool trait, ToolEvent, ToolStream
│       └── base_shell/
│           └── mod.rs      # BashShell 工具实现
└── target/                 # 编译输出
```

---

## 7. 依赖清单分析

| 依赖 | 用途 |
|------|------|
| `tokio` | 异步运行时，用于 HTTP 请求、子进程管理、Channel 通信 |
| `reqwest` | HTTP 客户端，调用 LLM API（SSE 流式） |
| `serde` / `serde_json` | 序列化/反序列化，处理消息格式、工具参数 |
| `futures-util` | 流处理工具，`StreamExt`、`FuturesUnordered` |
| `tokio-stream` | 将 `mpsc::Receiver` 包装为 `Stream` |
| `dotenvy` | 加载 `.env` 环境变量 |
| `clap` | CLI 参数解析（预留，暂未启用） |
| `tracing` | 日志追踪 |
| `anyhow` | 错误处理 |
| `jsonschema` | JSON Schema 校验（预留） |
| (无新增) | 权限沙箱零外部依赖，自实现 Glob 匹配、Shell 解析、TOML 解析 |
| `bytes` | 字节缓冲区处理 |

---

## 8. 扩展指南

### 8.1 添加新的 Model 适配器

```rust
pub struct MyModelAdapter { ... }

impl ModelAdapter for MyModelAdapter {
    fn stream_chat(&self, messages: &Vec<ChatMessage>, tools: serde_json::Value) -> ModelStream {
        // 实现自己的流式对话逻辑
    }
}
```

然后在 `main.rs` 的 `initial_model()` 中替换即可。

### 8.2 添加新的 Tool

```rust
pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "我的自定义工具" }
    fn parameters_schema(&self) -> serde_json::Value { ... }
    fn execute(&self, args: serde_json::Value) -> ToolStream { ... }
}
```

然后在 `main.rs` 的 `initial_tool_manager()` 中注册：

```rust
tool_manager.register_tool(Box::new(MyTool));
```

---

## 9. 当前局限与未来展望

| 方面 | 当前状态 | 改进方向 |
|------|---------|---------|
| 模型支持 | 仅 OpenAI 兼容 | 可新增 Anthropic、Google 等适配器 |
| 对话管理 | 单轮累加，无记忆管理 | 可引入上下文窗口管理、摘要机制 |
| 工具集 | 仅 BashShell | 可扩展文件读写、网络请求、数据库查询等 |
| 用户界面 | 纯终端 | 可包装为 WebSocket 服务、Web UI |
| 配置 | 硬编码 + .env | 可引入配置文件系统 |
| agent.rs | 空文件 | 可抽象 Agent 结构，支持多 Agent 协作 |

---

> **文档生成时间**: 2025年  
> **文档维护**: 请随项目迭代同步更新

## 10. 权限沙箱层（2025年新增）

权限沙箱层是项目的安全基础设施，位于 `src/tools/permission/` 目录下。

### 设计理念

> **不是为每个操作自定义工具，而是给 shell 工具戴上安全缰绳。**

### 架构

```
PermissionWrapper (装饰器，包装任何 Tool)
    │
    ├── ParsedCommand (命令解析器)
    │   ├── 提取命令名、参数、文件路径、URL
    │   └── 支持引号处理（单引号/双引号）
    │
    ├── PermissionChecker (检查引擎)
    │   ├── ① 命令黑名单：禁止高危命令（sudo, dd, reboot...）
    │   ├── ② Sudo 检查：禁止提权
    │   ├── ③ 敏感文件保护：.env, .git/, *.pem...
    │   ├── ④ 路径白名单：只能在项目目录内操作
    │   └── ⑤ 网络检查：默认禁止外部网络请求
    │
    └── CheckResult (结构化拒绝反馈)
        ├── { ok, error: { code, rule, message, suggestion } }
        └── 模型可读，自动寻找替代方案
```

### 特性

- **零外部依赖**：Glob 匹配、Shell 命令解析、TOML 配置解析均为自实现
- **装饰器模式**：不侵入 BashShell 代码，可包装任意工具
- **规则注入提示词**：自动生成权限摘要注入系统提示词
- **可配置**：通过 `config.toml` 自定义规则，无需改代码
- **37 个单元测试**，覆盖所有核心场景

