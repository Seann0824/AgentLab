# 🤖 Agent Lab — 自我进化的 AI Agent 框架

> **一个用 Rust 编写的、由 LLM 驱动的自主 Agent 框架。**  
> 核心设计理念：Agent 不仅能完成你交给的任务，还能**修改自身代码**来获得新能力。

[![Rust](https://img.shields.io/badge/Rust-2024-edition?logo=rust)](https://www.rust-lang.org/)
[![DeepSeek](https://img.shields.io/badge/LLM-DeepSeek-4A90D9)](https://deepseek.com)

---

## 📋 目录

- [为什么是 Agent Lab？](#-为什么是-agent-lab)
- [核心架构](#-核心架构)
- [快速开始](#-快速开始)
- [内置工具](#-内置工具)
- [上下文管理](#-上下文管理)
- [结构化任务执行](#-结构化任务执行)
- [模型适配](#-模型适配)
- [项目结构](#-项目结构)
- [配置参考](#-配置参考)
- [Roadmap](#-roadmap)

---

## 🎯 为什么是 Agent Lab？

大多数 AI Agent 框架是**黑盒** —— Agent 调用工具，但你无法让 Agent 给自己加一个新工具。  
Agent Lab 打破了这一限制：

| 特性 | 传统 Agent 框架 | Agent Lab |
|------|----------------|-----------|
| 工具调用 | ✅ 支持 | ✅ 支持 |
| 自我进化 | ❌ 不能修改自身 | ✅ 可增删改自身工具和能力 |
| 上下文窗口管理 | ❌ 简单截断 | ✅ 四层渐进压缩 |
| 状态持久化 | ❌ 依赖对话记忆 | ✅ 文件化 + 自动注入 |
| 结构化任务执行 | ❌ 依靠提示词 | ✅ TaskManager 框架 |

> **一句话：Agent Lab 是「会写自己代码」的 Agent 框架。**

---

## 🏗️ 核心架构

```
┌─────────────────────────────────────────────────────┐
│                    main.rs (入口)                     │
│  ┌──────────┐  ┌──────────┐  ┌───────────────────┐  │
│  │ Tool     │  │ Model    │  │ ContextManager    │  │
│  │ Manager  │  │ Adapter  │  │ (上下文管理器)     │  │
│  └────┬─────┘  └────┬─────┘  └────────┬──────────┘  │
│       │              │                 │             │
│  ┌────▼─────┐  ┌────▼─────┐  ┌────────▼──────────┐ │
│  │  Tools   │  │  LLM     │  │  TaskManager      │ │
│  │  shell   │  │  DeepSeek│  │  PLAN.md          │ │
│  │  edit    │  │  OpenAI  │  │  AGENDA.md        │ │
│  │  read    │  │  ...     │  │  MEMORY.md        │ │
│  │  search  │  │          │  │                   │ │
│  └──────────┘  └──────────┘  └───────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 核心组件

| 组件 | 文件 | 职责 |
|------|------|------|
| **ToolManager** | `src/tools/mod.rs` | 工具注册、调度、参数校验、结果流式返回 |
| **ContextManager** | `src/context/mod.rs` | 消息生命周期、Token 估算、四层渐进压缩 |
| **TaskManager** | `src/task/mod.rs` | 结构化任务状态管理、文件持久化、压缩恢复注入 |
| **ModelAdapter** | `src/model/mod.rs` | LLM 模型抽象层，支持流式对话 |
| **OpenAiCompatibleAdapter** | `src/model/openai_compatible.rs` | OpenAI 兼容 API 实现（DeepSeek） |

---

## 🚀 快速开始

### 前置条件

- Rust 2024 edition
- 一个兼容 OpenAI API 的 LLM（推荐 DeepSeek）

### 1. 配置环境变量

```bash
# .env
DEEPSEEK_API_KEY=your_api_key_here
DEEPSEEK_BASE_URL=https://api.deepseek.com/v1
```

### 2. 运行

```bash
cargo run
```

启动后会进入交互模式，你输入指令，Agent 自动规划和执行。

### 示例

```
> 帮我看看项目结构
━━━ 🔧 调用工具: shell
  $ find . -type f -name "*.rs" | head -20
━━━ ✅ 执行成功 (exit: 0)
...
> 搜索一下代码里哪里用到了 "pruning"
━━━ 🔧 调用工具: search
  pattern: pruning
━━━ ✅ 执行成功 (exit: 0)
...
```

---

## 🔧 内置工具

Agent Lab 目前内置 4 个工具，每个都是通过 `Tool trait` 实现的独立模块。

### 1. `shell` — 命令行执行

运行本地 CLI 命令（通过 zsh）。

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `command` | string | ✅ | 要执行的 shell 命令 |

```json
{
  "ok": true,
  "result": {
    "command": "ls -la",
    "status": 0,
    "success": true,
    "stdout": "...",
    "stderr": ""
  }
}
```

**特性**：
- 30 分钟超时保护
- stdout/stderr 分离
- 非零退出码不抛异常（返回 `success: false`）

### 2. `edit` — 增量文件编辑

对文件进行**增量**修改，而非全量重写（节省 Token）。

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `file_path` | string | ✅ | 要编辑的文件路径 |
| `operation` | string | ✅ | 操作类型：`search_replace` / `insert` / `delete` / `append` |
| `search` | string | 条件必填 | 精确匹配的搜索文本 |
| `replace` | string | 条件必填 | 替换后的新文本 |
| `content` | string | 条件必填 | 插入/追加的内容 |
| `line` | integer | 可选 | 插入位置的行号 |
| `mode` | string | 可选 | `before` / `after`（默认 after） |
| `dry_run` | boolean | 可选 | 预览模式，不实际修改 |

**设计原则**：
- 只发送变更部分，不发送整个文件
- 搜索文本必须精确匹配（包括空格和缩进）
- 支持 `dry_run` 预览 diff

### 3. `read` — 文件读取

读取文件内容，支持行号范围和行号显示。

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `file_path` | string | ✅ | 要读取的文件路径 |
| `start_line` | integer | 可选 | 起始行号（1-based） |
| `end_line` | integer | 可选 | 结束行号（1-based） |
| `show_line_numbers` | boolean | 可选 | 是否显示行号（默认 true） |
| `max_length` | integer | 可选 | 最大输出字符数（默认 5000） |

### 4. `search` — 目录文本搜索

在目录中搜索文本内容，支持正则表达式和文件过滤。

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `pattern` | string | ✅ | 搜索模式 |
| `path` | string | 可选 | 搜索起始目录（默认当前目录） |
| `include_ext` | string | 可选 | 只搜索指定扩展名，如 `"rs,toml,md"` |
| `exclude_dirs` | string | 可选 | 排除的目录（默认 `target,.git,.agent,node_modules`） |
| `regex` | boolean | 可选 | 是否使用正则（默认 false） |
| `ignore_case` | boolean | 可选 | 是否忽略大小写（默认 true） |
| `max_results` | integer | 可选 | 最大匹配行数（默认 50） |
| `context_lines` | integer | 可选 | 上下文的行数（默认 0） |

---

## 🧠 上下文管理

这是 Agent Lab 的核心竞争力之一。ContextManager 实现了**四层渐进压缩策略**，在保持对话质量的同时严格控制 Token 消耗。

### 压缩层级

```
层0: 工具调用结果修剪（最轻量）
  └─ 将早期工具的长输出替换为占位符
  └─ 保留对话结构：谁调用了什么工具 -> 结果（截断）

层1: 滑动窗口
  └─ 删除最旧的完整对话轮次
  └─ 保护系统提示词 + 标记为 preserved 的消息

层2: 异步摘要（LLM 生成）
  └─ 将早期对话压缩为结构化摘要
  └─ 格式：「目标 → 操作 → 决策 → 状态」

层3: 保底截断（极端情况）
  └─ 删除非保护消息直到 token 达标
```

### Token 估算优化

- **增量缓存**：添加消息时只计算新消息的 Token，O(1) 更新
- **双重降级**：优先使用 `tiktoken-rs` (cl100k_base)，不可用时用字符统计经验公式（±20%）
- **校准机制**：支持对特定模型的校准系数

### 消息重要性

自动分类消息，保护关键上下文不被压缩：

| 级别 | 说明 | 示例 |
|------|------|------|
| `Normal` | 普通对话 | 日常问答 |
| `Important` | 关键上下文 | 文件读取结果、项目结构发现 |
| `Milestone` | 里程碑决策 | 方案选择、架构决策 |

---

## 📋 结构化任务执行

Agent 遵循 **PLAN → EXECUTE → VERIFY → SUMMARY** 的工作流，相关状态持久化到文件中。

### 状态文件

| 文件 | 用途 | 生命周期 |
|------|------|---------|
| `docs/PLAN.md` | 当前执行计划（步骤列表 + 完成状态） | 每任务 |
| `docs/AGENDA.md` | 当前议程精简版 | 每任务 |
| `docs/MEMORY.md` | 重要发现、关键决策、已知问题 | 跨任务 |

### 工作流程

```
1. 🧠 规划 → 分析需求，创建 docs/PLAN.md
2. 🔧 执行 → 按步骤执行，每完成一步更新状态
3. ✅ 验证 → 修改后必须 cargo check，失败 3 次则重新规划
4. 📝 总结 → 汇报完成内容、关键决策、项目状态
```

### 上下文恢复

当上下文窗口被压缩后，TaskManager 会自动将当前任务状态注入到对话中，使 Agent 能够「记住」进行到哪一步了。

```
📋 【当前任务状态】
  任务: 实现搜索工具
  进度: 60% (3/5)

  ✅ 已完成:
    - [x] 实现 SearchTool 结构体
    - [x] 实现 execute 方法
    - [x] 注册到 ToolManager

  ⏳ 待完成:
    - [ ] 添加文件扩展名过滤
    - [ ] 编写单元测试
  ▶️ 当前步骤: 添加文件扩展名过滤
```

---

## 🔌 模型适配

通过 `ModelAdapter` trait 抽象 LLM 调用，可轻松切换后端。

### 当前支持

- **DeepSeek**（默认，通过 `OpenAiCompatibleAdapter`）
- 任何兼容 OpenAI Chat API 的模型（OpenAI、Claude via AWS、通义千问等）

### 添加新模型

实现 `ModelAdapter` trait：

```rust
pub trait ModelAdapter: Send + Sync {
    fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: serde_json::Value,
    ) -> ModelStream;
}
```

然后在 `main.rs` 的 `initial_model()` 中替换即可。

---

## 📁 项目结构

```
agent-lab/
├── Cargo.toml                  # 项目配置与依赖
├── Cargo.lock
├── .env                        # 环境变量（API Key 等）
├── README.md                   # 本文件
├── docs/
│   ├── PLAN.md                 # 当前执行计划
│   ├── AGENDA.md               # 当前议程
│   ├── MEMORY.md               # 重要发现记录
│   ├── index.md                # 文档索引
│   ├── analyses/               # 分析文档
│   ├── archive/                # 历史归档
│   └── designs/                # 设计文档
├── src/
│   ├── main.rs                 # CLI 入口与 Agent 启动
│   ├── agent/                  # Agent 主循环、命令、渲染、Goal 自动推进
│   ├── context/
│   │   ├── mod.rs             # ContextManager — 上下文生命周期管理
│   │   ├── config.rs          # ContextStrategy 配置类型
│   │   ├── strategy/          # 四层压缩策略实现
│   │   ├── summarizer.rs      # 异步/规则摘要生成器
│   │   ├── tokenizer.rs       # Token 估算器（tiktoken + 降级）
│   │   └── types.rs           # 上下文相关数据类型
│   ├── goal/                   # Goal 生命周期与持久化
│   ├── model/
│   │   ├── mod.rs             # 模型模块入口
│   │   ├── types.rs           # ModelAdapter trait 及事件类型
│   │   └── openai_compatible.rs # OpenAI 兼容 API 实现
│   ├── session/                # 会话保存与恢复
│   ├── swarm/                  # 多 Agent 通信、注册、池与工作流
│   ├── task/
│   │   ├── mod.rs             # TaskManager — 结构化任务执行
│   │   └── types.rs           # 任务状态数据类型
│   └── tools/
│       ├── mod.rs             # ToolManager — 工具注册与调度
│       ├── types.rs           # Tool trait 定义
│       ├── shell/              # shell 工具实现
│       ├── edit/               # edit 工具实现
│       ├── read/               # read 工具实现
│       └── search/             # search 工具实现
└── target/                    # 编译产物
```

### 核心依赖

| 依赖 | 用途 |
|------|------|
| `tokio` | 异步运行时（全功能模式） |
| `reqwest` | HTTP 客户端（SSE 流式请求） |
| `serde / serde_json` | 序列化/反序列化 |
| `tiktoken-rs` | OpenAI cl100k_base Token 计数 |
| `jsonschema` | 工具参数 schema 校验 |
| `walkdir` | 目录递归遍历 |
| `regex` | 文本搜索正则支持 |
| `clap` | CLI 参数解析 |
| `tracing` | 日志追踪 |

---

## ⚙️ 配置参考

### 上下文策略

通过 `AgentConfig` 转换为 `ContextStrategy::Auto` 后注入 `ContextManager`：

```rust
let strategy = ContextStrategy::Auto {
    token_limit: 128_000,              // Token 硬限制
    max_turns: 20,                     // 滑动窗口保留轮数
    trigger_ratio: 0.7,               // 触发压缩的阈值（70%）
    enable_async_summary: true,        // 启用异步 LLM 摘要
    enable_tool_pruning: true,         // 启用工具结果修剪
    tool_pruning_keep_recent: 3,       // 保留最近 3 轮工具结果
    tool_pruning_max_output_chars: 200, // 工具输出超过 200 字符即修剪
};
```

### 模型配置

通过 `.env` 文件配置：

```env
DEEPSEEK_API_KEY=your_key
DEEPSEEK_BASE_URL=https://api.deepseek.com/v1
# ModelManager 会从环境变量发现兼容 OpenAI API 的模型配置
```

---

## 🛤️ Roadmap

- [x] 基础工具系统（shell / edit / read）
- [x] 上下文窗口管理（四层渐进压缩）
- [x] 结构化任务执行框架
- [x] 文件搜索工具（search）
- [x] 异步摘要生成
- [ ] 多会话支持（session 管理）
- [ ] 权限控制（策略引擎）
- [ ] 工作流编排（多 Agent 协作）
- [ ] 向量记忆（长期记忆）
- [ ] Web UI 界面
- [ ] 插件系统（动态加载工具）
- [ ] Docker 部署支持

---

## 🧪 开发指南

### 添加新工具

1. 在 `src/tools/` 下创建新模块目录
2. 实现 `Tool` trait
3. 在 `src/tools/mod.rs` 中注册
4. 在 `src/agent/default_tools.rs` 的默认工具管理器中注册

```rust
// 1. 定义工具
pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "我的自定义工具" }
    fn parameters_schema(&self) -> serde_json::Value { /* JSON Schema */ }
    fn execute(&self, args: serde_json::Value) -> ToolStream {
        // 返回异步流
    }
}

// 2. 注册
tool_manager.register_tool(Box::new(MyTool));
```

### 验证修改

```bash
cargo check 2>&1 | tail -30
```

---

## 📄 许可证

MIT License

---

<div align="center">
  <p><em>让你的 Agent 自己写代码，而不是等别人来写。</em></p>
  <p>Built with 🦀 Rust</p>
</div>
