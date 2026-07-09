<!-- From: /Users/sean/Desktop/repo/agent-lab/AGENTS.md -->
# agent-lab 项目指南

> 本文件面向 AI 编程助手。阅读前默认不了解本项目，所有信息均基于当前仓库实际内容整理，不做外部假设。

## 1. 项目概述

`agent-lab` 是一个用 Rust 编写的 AI Agent 实验项目。当前定位：

- `agent-lab-core` 是整个应用的**核心库**，承载 Agent、工具、记忆、RAG、会话/消息持久化等业务能力；它不是对外通用的框架，而是为 `agent-lab-desktop` 及未来可能的 web/app 应用提供统一核心。
- `agent-lab-desktop` 是基于 Tauri 的**桌面端胶水层**，负责 UI 状态管理、用户交互以及把 core 的能力通过 commands 暴露给前端。
- 仓库同时保留若干可独立运行的示例（`rag_agent`、`novel_generation`），用于验证和演示 core 能力。

当前状态：

- 这是一个实验性仓库，部分功能已实现并可通过 `cargo` 编译运行，部分接口（如 MemoryManager 的部分管理方法）仍是半成品或带有 `TODO`。
- `crates/agent-lab-core/src/main.rs` 已删除，`agent-lab-core` 现在是纯 lib。
- 代码注释以中文为主；标识符、模块名使用英文。

## 2. 技术栈

- **语言**：Rust， edition = `"2024"`
- **异步运行时**：Tokio（`full` feature）
- **桌面端**：Tauri v2 + React + Windi CSS / Tailwind CSS
- **LLM 客户端**：基于 `openai-api-rs`（workspace 成员，位于 `crates/openai-api-rs`），支持任意 OpenAI 兼容接口（如 DeepSeek、OpenRouter、自建服务等）
- **数据库**：
  - PostgreSQL + `pgvector` 扩展：记忆向量、RAG chunk 向量、会话与消息持久化
  - Neo4j：存储记忆的实体-关系引用图
- **持久化访问**：`sqlx`（异步、编译时检查）
- **Embedding**：默认通过 HTTP 调用 Ollama（`nomic-embed-text`），可配置模型和地址
- **向量搜索**：`pgvector` HNSW 索引 + cosine distance
- **Web 搜索**：SerpApi
- **序列化/配置**：`serde`、`serde_json`、`dotenvy`
- **日志**：`tracing`

> 注意：仓库根目录存在一个 `node_modules/`，属于历史遗留。现在根目录另有 `package.json`，仅作为 Rust / Tauri 任务的聚合入口（见第 4 节），不参与 Rust 编译。

## 3. 项目结构

```text
Cargo.toml                   # Rust workspace 根
Cargo.lock                   # 共享 lock 文件
package.json                 # 任务聚合入口（pnpm lab / novel / desktop 等）
.cargo/config.toml           # cargo alias（lab / novel / c / t 等）
crates/
├── agent-lab-core/          # 核心库
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs           # 库入口：导出 agent / tools / base / services / storage / error
│   │   ├── db.rs            # 全局 PostgreSQL 连接池 get_db_client
│   │   ├── error.rs         # 统一错误类型 AgentLabError
│   │   ├── base/            # 基础抽象
│   │   │   ├── agent.rs     # AgentBase、Agent trait、AgentStreamEvent
│   │   │   ├── config.rs    # Config（环境变量 + 默认值）
│   │   │   └── llm.rs       # AgentsLLM（OpenAI 兼容客户端封装）
│   │   ├── agent/           # Agent 实现
│   │   │   ├── simple_agent.rs
│   │   │   ├── react_agent.rs
│   │   │   ├── reflection_agent.rs
│   │   │   ├── tool_agent.rs
│   │   │   ├── group_chat.rs
│   │   │   └── mod.rs
│   │   ├── tools/           # 工具系统
│   │   │   ├── mod.rs
│   │   │   ├── types.rs
│   │   │   ├── rag_tool.rs
│   │   │   ├── web_search/mod.rs
│   │   │   └── memory/...
│   │   ├── services/        # 业务服务层
│   │   │   ├── chat_service.rs    # 会话生命周期 + Agent 运行编排
│   │   │   ├── session_service.rs # 会话 CRUD
│   │   │   ├── message_service.rs # 消息 CRUD
│   │   │   ├── memory_service.rs
│   │   │   ├── rag_service.rs
│   │   │   ├── chat_dto.rs        # ChatMessage / SessionSummary / ToolCallInfo
│   │   │   ├── error.rs
│   │   │   └── mod.rs
│   │   └── storage/         # 存储层
│   │       ├── chat_store.rs      # chat_sessions / chat_messages CRUD
│   │       ├── pg.rs
│   │       ├── neo4j.rs
│   │       ├── embedder/...
│   │       └── mod.rs
│   ├── examples/
│   │   ├── rag_agent.rs           # 终端交互式 RAG Agent
│   │   └── novel_generation.rs    # 多 Agent 小说创作
│   └── tests/
│       ├── integration_tests.rs
│       └── tools/...
├── openai-api-rs/           # OpenAI 兼容客户端（workspace 成员）
│   └── src/v1/...
└── apps/
    └── agent-lab-desktop/   # Tauri 桌面端
        ├── src/             # React 前端
        │   ├── App.tsx
        │   ├── api/chatApi.ts
        │   ├── components/  # MessageList / MessageItem / ChatInput 等
        │   ├── store/chatStore.ts
        │   ├── types/chat.ts
        │   └── styles/...
        └── src-tauri/       # Tauri Rust 后端
            ├── src/
            │   ├── main.rs
            │   ├── lib.rs
            │   ├── state.rs
            │   ├── commands/chat.rs   # Tauri command 胶水层
            │   └── services/chat.rs   # core ChatService 桥接
            └── Cargo.toml

init_pg.sql                  # PG 建表/索引脚本
.env                         # 环境变量（已加入 .gitignore，勿提交）
```

## 4. 构建与运行

### 4.1 基础命令

推荐通过根目录 `package.json` 运行常用任务（需先安装 pnpm）：

```bash
# 运行桌面端（Tauri dev）——当前主入口
pnpm desktop

# 构建桌面端
pnpm desktop:build

# 运行终端 RAG Agent 示例
pnpm lab

# 运行示例（多 Agent 小说创作）
pnpm novel

# 检查 workspace
pnpm check

# 运行库单元测试
pnpm test
```

等价原生 cargo 命令（也已在 `.cargo/config.toml` 中配置为 alias）：

```bash
# 检查
cargo check          # 或 cargo c

# 构建
cargo build

# 构建 Release
cargo build --release

# 运行终端 RAG Agent 示例
cargo run -p agent-lab-core --example rag_agent    # 或 cargo lab

# 运行小说示例
cargo run -p agent-lab-core --example novel_generation    # 或 cargo novel

# 运行测试
cargo test           # 或 cargo t

# 只运行库单元测试
cargo test --lib
```

当前 `cargo check` / `cargo test --no-run` 均能通过，但会报若干 `unused` / `async_fn_in_trait` 等 warning。这是已知状态，不影响编译。

### 4.2 运行前必须准备的环境

1. **PostgreSQL + pgvector**：执行 `init_pg.sql` 创建表和索引。
2. **Neo4j**（用于记忆的实体关系图）。
3. **Ollama**：默认在 `http://localhost:11434/api/embeddings` 运行 `nomic-embed-text`（维度 768）。
4. **OpenAI 兼容 LLM 服务**：如 DeepSeek、OpenRouter 等。
5. **SerpApi**（仅使用 `web_search` 工具时需要）。

## 5. 配置与环境变量

项目使用 `dotenvy` 加载 `.env`。`.env` 已加入 `.gitignore`，**不能提交到版本库**。

### 5.1 LLM 基础配置（`AgentsLLM::from_env`）

| 变量 | 说明 |
|------|------|
| `API_KEY` | LLM API Key（必填） |
| `BASE_URL` | LLM 接口 base URL（必填） |
| `MODEL` | 模型名（必填） |
| `PROVIDER` | 提供商标识，默认 `Custom` |

### 5.2 记忆/RAG/会话数据库

| 变量 | 说明 |
|------|------|
| `DATABASE_URL` | PostgreSQL 连接串，例如 `postgres://user:pass@localhost/db` |
| `NEO4J_URL` | Neo4j Bolt URI，默认 `neo4j://127.0.0.1:7687` |
| `NEO4J_USER` | 默认 `neo4j` |
| `NEO4J_PASSWORD` | |

### 5.3 Embedding

| 变量 | 说明 |
|------|------|
| `EMBEDDER_URL` | Ollama embeddings 端点，默认 `http://localhost:11434/api/embeddings` |
| `EMBEDDER_MODEL` | 默认 `nomic-embed-text` |

### 5.4 其他可选配置

| 变量 | 说明 |
|------|------|
| `SERPAPI_API_KEY` | WebSearch 工具使用 |
| `DEFAULT_MODEL` / `DEFAULT_PROVIDER` | `Config` 默认值覆盖 |
| `TEMPERATURE` / `MAX_TOKENS` / `DEBUG` | `Config::from_env` 读取 |
| `DEEPSEEK_API_KEY` / `DEEPSEEK_BASE_URL` / `DEEPSEEK_MODEL` | 仅 `crates/agent-lab-core/examples/novel_generation.rs` 使用 |

## 6. 数据库初始化

运行记忆、RAG 或聊天功能前，先对目标 PG 执行：

```bash
psql $DATABASE_URL -f init_pg.sql
```

`init_pg.sql` 会：

- 启用 `vector` 扩展
- 创建 `memories` 表（向量维度 768）
- 创建 `rag_chunks` 表（向量维度 768）
- 创建 `chat_sessions` 表与 `chat_messages` 表（消息持久化）
- 创建 HNSW 向量索引和常用 BTree 索引

> 当前代码中 embedding 维度固定为 768（与 `init_pg.sql` 一致）。若更换 embedder，必须同步修改表结构和代码中的 `dimension`。

## 7. 核心模块说明

### 7.1 Agent

- `AgentBase`：统一持有 name、llm、system_prompt、config、history 与 event_sender。
- `Agent` trait：要求实现 `base()` / `base_mut()` 和 `run()`。
- `SimpleAgent`：最通用的流式对话 Agent，支持 tool calling 循环；当前桌面端 `ChatService` 使用它。
- `ReActAgent`：与 SimpleAgent 类似，但内置 `max_steps` 限制。
- `ReflectionAgent`：支持三段式 prompt（initial / reflect / refine）迭代。
- `ToolAgent<T>`：强制模型调用工具，并将工具返回 JSON 反序列化为 `T`，失败自动重试 3 次。
- `RoundRobinGroupChat`：基于 `Agent` trait 的多 Agent 轮询群聊，供 `novel_generation` 示例使用。

### 7.2 流式事件

`AgentStreamEvent`（`crates/agent-lab-core/src/base/agent.rs`）是前后端之间的统一流式协议：

- `UserMessage { message }`：用户消息已加入历史并持久化。
- `AssistantDelta { message_id, delta }`：assistant 内容增量，携带该条消息的真实 id。
- `ReasonDelta { message_id, delta }`：reasoning/thinking 增量，与 assistant 消息共享同一 id。
- `AssistantDone { message }`：assistant 消息生成完毕。
- `ToolCallStart / ToolCallEnd / ToolCallDelta`：工具调用生命周期事件。

设计要点：后端在 assistant 回复开始时即生成真实 `message_id`，并随第一个 `ReasonDelta` 或 `AssistantDelta` 下发；前端基于该 id 维护流式消息，避免前后端 id 不一致导致重复渲染。

### 7.3 Services

业务服务层位于 `crates/agent-lab-core/src/services/`：

- `ChatService`：编排会话生命周期与 Agent 运行。每次发送消息时从 DB 加载历史、新建 Agent、运行、并通过事件持久化新消息。
- `SessionService`：会话 CRUD（创建、删除、重命名、列表、touch）。
- `MessageService`：消息 CRUD（按 session 查询历史、添加消息）。
- `RagService` / `MemoryService`：RAG 与记忆的业务封装。
- `chat_dto.rs`：定义 `ChatMessage`、`SessionSummary`、`ToolCallInfo` 等前后端共享 DTO。

### 7.4 Storage

存储层位于 `crates/agent-lab-core/src/storage/`：

- `ChatStore`：基于 `sqlx` 的 `chat_sessions` / `chat_messages` CRUD，被 `SessionService` / `MessageService` 使用。
- `PgStore`：记忆与 RAG chunk 的 PG 向量/结构化存储。
- `Neo4jStore`：记忆的实体-关系引用图。
- `Embedder` / `OllamaEmbedder`：文本向量化。
- `MemoryStore`：组合 PG + Neo4j + Embedder，对外提供记忆读写接口。

### 7.5 Tools

- `Tool` trait：`name()`、`description()`、`parameters_schema()`、`execute()`。
- `ToolManager`：通过 `HashMap<String, Box<dyn Tool + Send + Sync>>` 管理工具，生成 OpenAI function schema，并发执行 tool calls。
- 现有工具：
  - `RagTool`：Markdown 分块、索引、语义检索；内置 MQE + HyDE 增强。
  - `MemoryTool`：记忆增删改查、整合、遗忘。
  - `WebSearch`：SerpApi 网页搜索。
  - `BashShell`（`crates/agent-lab-core/src/tools/base_shell/mod.rs` 中，当前在 `tools/mod.rs` 被注释导出）：本地 shell 命令工具，**谨慎启用**。

### 7.6 Memory

记忆类型：

- `working`：进程内 TF-IDF + 关键词 + 时间衰减，容量/过期控制。
- `episodic`：PG 向量检索 + 关键词回退，加入时间/重要性综合排序。
- `semantic`：PG 向量 + Neo4j 实体图混合检索，抽取实体/关系。
- `perceptual`：进程内简单关键词匹配。

存储层：

- `MemoryStore` 组合 `PgStore` + `Neo4jStore` + `Embedder`。
- `PgStore` 保存完整记忆内容与向量。
- `Neo4jStore` 只保存引用：`(:Memory)-[:HAS_ENTITY]->(:Entity)-[:RELATED_TO]->(:Entity)`。实体 id 由 `name + type` 经 FNV-1a hash 生成，保证跨运行稳定。

### 7.7 RAG

- `RagTool` 暴露 `add_document` / `search` 两个 action。
- `RagIndex` 负责：
  - Markdown 按标题层级分段（`split_paragraphs_with_headings`）
  - 按 token 估算分块（`chunk_paragraphs`）
  - Markdown 清洗（`preprocess_markdown_for_embedding`）
  - Ollama 生成 embedding 后写入 `rag_chunks`
  - 检索时启用 HyDE + MQE，多查询召回后去重、重排

### 7.8 Desktop

`apps/agent-lab-desktop` 是当前主应用入口：

- 前端：`React` + `Windi CSS`，状态管理使用 `Zustand`（`store/chatStore.ts`）。
- 后端：`src-tauri/src/lib.rs` 在 `setup` 中构建 `AgentsLLM`、`ChatStore`、`SessionService`、`MessageService`、`ChatService`，并通过 `AppState` 共享给 commands。
- Commands：`src-tauri/src/commands/chat.rs` 提供 `chat_completion_stream`、`list_chat_sessions`、`get_chat_history`、`create_chat_session`、`delete_chat_session`、`rename_chat_session`。

## 8. 测试策略

### 8.1 测试命令

```bash
# 运行全部测试（部分需要 PG/Neo4j，否则会失败或跳过）
cargo test

# 只运行不依赖外部服务的库单元测试
cargo test --lib
```

### 8.2 测试分类

| 测试 | 位置 | 依赖 | 说明 |
|------|------|------|------|
| `test_new_embedder` | `crates/agent-lab-core/src/storage/embedder/ollama_embedder.rs` | 无 | 仅验证默认值 |
| `test_pg_store_add` | `crates/agent-lab-core/src/storage/pg.rs` | PG + DATABASE_URL | 真实写入/清理 |
| `test_chat_store_session_and_message_crud` | `crates/agent-lab-core/tests/integration_tests.rs` | PG + DATABASE_URL | 会话/消息 CRUD 集成测试 |
| `test_rag_*`（多数） | `crates/agent-lab-core/tests/tools/rag/rag_test.rs` | 无 | 纯文本处理与分块逻辑 |
| `test_rag_index_empty_chunks` | `crates/agent-lab-core/tests/tools/rag/rag_test.rs` | PG（可选） | 无 DATABASE_URL 时自动跳过 |
| `test_episodic_memory_add_and_retrieve` | `crates/agent-lab-core/tests/tools/memory/episodic_memory_test.rs` | PG + Neo4j | 集成测试 |
| `test_neo4j_reference_graph_crud` | `crates/agent-lab-core/tests/tools/memory/storage/neo4j_test.rs` | Neo4j | 集成测试 |

### 8.3 测试环境建议

- 本地开发时建议通过 docker-compose 启动 PG + pgvector 和 Neo4j。
- 没有外部服务时，`cargo test --lib` 可保证不依赖服务的用例通过。
- 新增持久化相关测试请放到 `tests/` 目录下，不要直接写在 `src/` 里。

## 9. 代码风格与约定

- 模块组织：领域驱动，`base` / `agent` / `tools` / `services` / `storage` / `db` 分层。
- 异步 trait：使用 `async-trait` crate（如 `Tool`、`Memory`）。
- LLM 调用：统一使用 `AgentsLLM`（`crates/agent-lab-core/src/base/llm.rs`），不再保留旧客户端。
- 工具 schema：使用 `openai_api-rs::v1::types::{FunctionParameters, JSONSchemaDefine, JSONSchemaType}` 构造，不要手写 JSON。
- 错误处理：
  - 业务层统一使用 `AgentLabError`（`crates/agent-lab-core/src/error.rs`）。
  - 存储层错误使用 `StorageError` / `ServiceError`，带 `[Module] reason` 前缀。
  - 错误应集中处理并暴露给调用方，避免到处 `map_err(|e| e.to_string())`。
- 注释：以中文为主，保留现有中文注释风格。
- 命名：存在少量拼写错误（如 `capacoty`、`scehma`、`chunck`、`blance`），修改时尽量顺带修正，但不要为了重命名而大改接口。
- Warning：当前仓库有较多 `unused` / `async_fn_in_trait` warning。修复时优先保证编译通过，再逐步清理。

## 10. 安全注意事项

- **API Key 与密码**：全部通过 `.env` 注入，`.env` 已在 `.gitignore` 中。**绝对不要将真实 key 写入源码或提交到 git**。
- **Shell 工具**：`crates/agent-lab-core/src/tools/base_shell/mod.rs` 中的 `BashShell` 当前被注释导出。启用它会让 Agent 获得执行本地 shell 的能力，必须配合严格的命令白名单/沙箱，否则风险极高。
- **SQL 注入**：存储层使用 `sqlx` 参数绑定，没有字符串拼接 SQL，基本安全。
- **Neo4j 注入**：Cypher 查询使用 `neo4rs::query().param()` 绑定，避免拼接。
- **第三方服务**：WebSearch 会把查询发送到 SerpApi；RAG/记忆 embedding 会发送到本地/远程 Ollama；LLM 调用会发送到配置的 `BASE_URL`。注意这些服务的合规与数据安全。

## 11. 已知问题与 TODO（基于代码实际内容）

- `MemoryManager` 的 `consolidate_memories` 仍为占位实现，待后续补充真正的记忆整合逻辑。
- `Config` 中的 `log_level`、`max_history_length` 等字段当前未使用。
- 多处 `std::io::Write::flush(...)` 返回值未处理，产生 `unused_must_use` warning。

## 12. 快速开始检查清单

1. `cargo check` 通过（或 `pnpm check`）。
2. 准备 PostgreSQL + pgvector，执行 `init_pg.sql`。
3. 准备 Neo4j 并启动。
4. 准备 Ollama 并拉取 `nomic-embed-text`。
5. 在 `.env` 中填入 `DATABASE_URL`、`API_KEY`、`BASE_URL`、`MODEL`、`NEO4J_PASSWORD`。
6. 安装前端依赖：`pnpm install`。
7. 启动桌面端：
   - `pnpm desktop`
8. 体验终端 RAG 示例：
   - `pnpm lab`，或
   - `cargo run -p agent-lab-core --example rag_agent`
9. 体验多 Agent 小说示例：
   - `pnpm novel`，或
   - `cargo run -p agent-lab-core --example novel_generation`
