# agent-lab 项目指南

> 本文件面向 AI 编程助手。阅读前默认不了解本项目，所有信息均基于当前仓库实际内容整理，不做外部假设。

## 1. 项目概述

`agent-lab` 是一个用 Rust 编写的 **AI Agent 实验框架/脚手架**，目标是提供可组合的大模型 Agent、工具（Tools）、记忆（Memory）与 RAG（检索增强生成）能力。

当前状态：

- 这是一个实验性仓库，部分功能已实现并可通过 `cargo` 编译运行，部分接口（如 PlanAndSolveAgent、MemoryManager 的部分管理方法）仍是半成品或带有 `TODO`。
- 项目同时包含一个可运行的主程序（`src/main.rs`，交互式 RAG Agent）和一个示例（`examples/novel_generation.rs`，多 Agent 小说创作）。
- 代码注释以中文为主；标识符、模块名使用英文。

## 2. 技术栈

- **语言**：Rust， edition = `"2024"`
- **异步运行时**：Tokio（`full` feature）
- **LLM 客户端**：基于 `openai-api-rs`（vendored 在 `vendor/openai-api-rs`），支持任意 OpenAI 兼容接口（如 DeepSeek、OpenRouter、自建服务等）
- **数据库**：
  - PostgreSQL + `pgvector` 扩展：存储记忆向量与 RAG chunk 向量
  - Neo4j：存储记忆的实体-关系引用图
- **Embedding**：默认通过 HTTP 调用 Ollama（`nomic-embed-text`），可配置模型和地址
- **向量搜索**：`pgvector` HNSW 索引 + cosine distance
- **Web 搜索**：SerpApi
- **序列化/配置**：`serde`、`serde_json`、`dotenvy`
- **日志**：`tracing`

> 注意：仓库根目录存在一个 `node_modules/`，但项目当前没有 `package.json` 或前端构建脚本，该目录是历史遗留，不参与当前 Rust 构建流程。

## 3. 项目结构

```text
src/
├── main.rs                  # 可执行入口：交互式 RAG Agent
├── lib.rs                   # 库入口，导出 agent / tools / base / db
├── db.rs                    # 全局 PostgreSQL 连接池 get_db_client
├── base/                    # 基础抽象
│   ├── agent.rs             # AgentBase、Agent trait
│   ├── config.rs            # Config（环境变量 + 默认值）
│   ├── llm.rs               # AgentsLLM（OpenAI 兼容客户端封装）
│   └── message.rs           # Message 包装（含 metadata、timestamp）
├── agent/                   # Agent 实现
│   ├── simple_agent.rs      # 基础流式对话 Agent，支持 tool calling
│   ├── react_agent.rs       # ReAct 风格循环，带 max_steps
│   ├── reflection_agent.rs  # 反射/迭代优化 Agent
│   ├── tool_agent.rs        # 强制单次/多次工具调用并反序列化结果
│   ├── group_chat.rs        # 基于 Agent trait 的 RoundRobinGroupChat
│   └── mod.rs               # 模块声明与 re-export
└── tools/                   # 工具系统
    ├── mod.rs               # ToolManager：注册、调度、schema 生成
    ├── types.rs             # Tool trait、ToolEvent
    ├── web_search/mod.rs    # WebSearch（SerpApi）
    ├── rag/                 # RAG 工具与索引
    │   ├── mod.rs           # 模块 re-export
    │   ├── tool.rs          # RagTool：Tool trait 实现
    │   ├── index.rs         # RagIndex：embedding、PG 写入/检索
    │   ├── chunking.rs      # Markdown 分段、按 token 分块
    │   ├── markdown.rs      # Markdown 清洗
    │   ├── retrieval.rs     # HyDE + MQE 检索增强
    │   ├── hyde.rs          # HyDE 子 Agent
    │   └── query_expansion.rs # MQE 查询扩展子 Agent
    └── memory/              # 记忆系统
        ├── base.rs          # MemoryItem、Memory trait
        ├── mod.rs           # MemoryTool、MemoryManager
        ├── working_memory.rs
        ├── episodic_memory.rs
        ├── semantic_memory.rs
        ├── perceptual_memory.rs
        ├── extractor.rs     # 实体/关系抽取子 Agent（EntityExtractorAgent）
        └── storage/         # 存储实现
            ├── mod.rs       # MemoryStore（PG + Neo4j + Embedder 组合）
            ├── graph.rs     # 图实体 id 生成等图相关 helper
            ├── pg.rs        # PgStore
            ├── neo4j.rs     # Neo4jStore
            └── embedder/    # OllamaEmbedder

examples/
└── novel_generation.rs      # 多 Agent 小说创作示例

tests/
├── integration_tests.rs     # 测试模块组织入口
└── tools/                   # 集成测试与单元测试
    ├── memory/...
    └── rag/rag_test.rs

init_pg.sql                  # PG 建表/索引脚本
.env                         # 环境变量（已加入 .gitignore，勿提交）
Cargo.toml
Cargo.lock
```

## 4. 构建与运行

### 4.1 基础命令

```bash
# 检查
cargo check

# 构建
cargo build

# 构建 Release
cargo build --release

# 运行主程序（交互式 RAG Agent）
cargo run

# 运行示例
cargo run --example novel_generation

# 运行测试
cargo test

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

### 5.2 记忆/RAG 数据库

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
| `DEEPSEEK_API_KEY` / `DEEPSEEK_BASE_URL` / `DEEPSEEK_MODEL` | 仅 `examples/novel_generation.rs` 使用 |

## 6. 数据库初始化

运行记忆或 RAG 功能前，先对目标 PG 执行：

```bash
psql $DATABASE_URL -f init_pg.sql
```

`init_pg.sql` 会：

- 启用 `vector` 扩展
- 创建 `memories` 表（向量维度 768）
- 创建 `rag_chunks` 表（向量维度 768）
- 创建 HNSW 向量索引和常用 BTree 索引

> 当前代码中 embedding 维度固定为 768（与 `init_pg.sql` 一致）。若更换 embedder，必须同步修改表结构和代码中的 `dimension`。

## 7. 核心模块说明

### 7.1 Agent

- `AgentBase`：统一持有 name、llm、system_prompt、config、history。
- `Agent` trait：要求实现 `base()` / `base_mut()` 和 `run()`。
- `SimpleAgent`：最通用的流式对话 Agent，支持 tool calling 循环。
- `ReActAgent`：与 SimpleAgent 类似，但内置 `max_steps` 限制。
- `ReflectionAgent`：支持三段式 prompt（initial / reflect / refine）迭代。
- `ToolAgent<T>`：强制模型调用工具，并将工具返回 JSON 反序列化为 `T`，失败自动重试 3 次。
- `RoundRobinGroupChat`：基于 `Agent` trait 的多 Agent 轮询群聊，供 `novel_generation` 示例使用。

### 7.2 Tools

- `Tool` trait：`name()`、`description()`、`parameters_schema()`、`execute()`。
- `ToolManager`：通过 `HashMap<String, Box<dyn Tool + Send + Sync>>` 管理工具，生成 OpenAI function schema，并发执行 tool calls。
- 现有工具：
  - `RagTool`：Markdown 分块、索引、语义检索；内置 MQE + HyDE 增强。
  - `MemoryTool`：记忆增删改查、整合、遗忘。
  - `WebSearch`：SerpApi 网页搜索。
  - `BashShell`（`src/tools/base_shell/mod.rs` 中，当前在 `tools/mod.rs` 被注释导出）：本地 shell 命令工具，**谨慎启用**。

### 7.3 Memory

记忆类型：

- `working`：进程内 TF-IDF + 关键词 + 时间衰减，容量/过期控制。
- `episodic`：PG 向量检索 + 关键词回退，加入时间/重要性综合排序。
- `semantic`：PG 向量 + Neo4j 实体图混合检索，抽取实体/关系。
- `perceptual`：进程内简单关键词匹配。

存储层：

- `MemoryStore` 组合 `PgStore` + `Neo4jStore` + `Embedder`。
- `PgStore` 保存完整记忆内容与向量。
- `Neo4jStore` 只保存引用：`(:Memory)-[:HAS_ENTITY]->(:Entity)-[:RELATED_TO]->(:Entity)`。实体 id 由 `name + type` 经 FNV-1a hash 生成，保证跨运行稳定。

### 7.4 RAG

- `RagTool` 暴露 `add_document` / `search` 两个 action。
- `RagIndex` 负责：
  - Markdown 按标题层级分段（`split_paragraphs_with_headings`）
  - 按 token 估算分块（`chunk_paragraphs`）
  - Markdown 清洗（`preprocess_markdown_for_embedding`）
  - Ollama 生成 embedding 后写入 `rag_chunks`
  - 检索时启用 HyDE + MQE，多查询召回后去重、重排
- 默认 namespace 为 `default`，`main.rs` 中硬编码使用 `figma_agent`。

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
| `test_new_embedder` | `src/tools/memory/storage/embedder/ollama_embedder.rs` | 无 | 仅验证默认值 |
| `test_pg_store_add` | `src/tools/memory/storage/pg.rs` | PG + DATABASE_URL | 真实写入/清理 |
| `test_rag_*`（多数） | `tests/tools/rag/rag_test.rs` | 无 | 纯文本处理与分块逻辑 |
| `test_rag_index_empty_chunks` | `tests/tools/rag/rag_test.rs` | PG（可选） | 无 DATABASE_URL 时自动跳过 |
| `test_episodic_memory_add_and_retrieve` | `tests/tools/memory/episodic_memory_test.rs` | PG + Neo4j | 集成测试 |
| `test_neo4j_reference_graph_crud` | `tests/tools/memory/storage/neo4j_test.rs` | Neo4j | 集成测试 |

### 8.3 测试环境建议

- 本地开发时建议通过 docker-compose 启动 PG + pgvector 和 Neo4j。
- 没有外部服务时，`cargo test --lib` 可保证不依赖服务的用例通过。

## 9. 代码风格与约定

- 模块组织：领域驱动，`base` / `agent` / `tools` / `db` 分层。
- 异步 trait：使用 `async-trait` crate（如 `Tool`、`Memory`）。
- LLM 调用：统一使用 `AgentsLLM`（`src/base/llm.rs`），不再保留旧客户端。
- 工具 schema：使用 `openai_api-rs::v1::types::{FunctionParameters, JSONSchemaDefine, JSONSchemaType}` 构造，不要手写 JSON。
- 错误处理：大量使用 `Result<T, String>`；存储层错误带 `[Module] reason` 前缀。
- 注释：以中文为主，保留现有中文注释风格。
- 命名：存在少量拼写错误（如 `capacoty`、`scehma`、`chunck`、`blance`），修改时尽量顺带修正，但不要为了重命名而大改接口。
- Warning：当前仓库有较多 `unused` / `async_fn_in_trait` warning。修复时优先保证编译通过，再逐步清理。

## 10. 安全注意事项

- **API Key 与密码**：全部通过 `.env` 注入，`.env` 已在 `.gitignore` 中。**绝对不要将真实 key 写入源码或提交到 git**。
- **Shell 工具**：`src/tools/base_shell/mod.rs` 中的 `BashShell` 当前被注释导出。启用它会让 Agent 获得执行本地 shell 的能力，必须配合严格的命令白名单/沙箱，否则风险极高。
- **SQL 注入**：存储层使用 `sqlx` 参数绑定，没有字符串拼接 SQL，基本安全。
- **Neo4j 注入**：Cypher 查询使用 `neo4rs::query().param()` 绑定，避免拼接。
- **第三方服务**：WebSearch 会把查询发送到 SerpApi；RAG/记忆 embedding 会发送到本地/远程 Ollama；LLM 调用会发送到配置的 `BASE_URL`。注意这些服务的合规与数据安全。

## 11. 已知问题与 TODO（基于代码实际内容）

- `MemoryManager` 的 `consolidate_memories` 仍为占位实现，待后续补充真正的记忆整合逻辑。
- `Config` 中的 `log_level`、`max_history_length` 等字段当前未使用。
- 多处 `std::io::Write::flush(...)` 返回值未处理，产生 `unused_must_use` warning。
- 多处 `std::io::Write::flush(...)` 返回值未处理，产生 `unused_must_use` warning。

## 12. 快速开始检查清单

1. `cargo check` 通过。
2. 准备 PostgreSQL + pgvector，执行 `init_pg.sql`。
3. 准备 Neo4j 并启动。
4. 准备 Ollama 并拉取 `nomic-embed-text`。
5. 在 `.env` 中填入 `DATABASE_URL`、`API_KEY`、`BASE_URL`、`MODEL`、`NEO4J_PASSWORD`。
6. `cargo run` 启动交互式 RAG Agent，或 `cargo run --example novel_generation` 体验多 Agent 示例。
