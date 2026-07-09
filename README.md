# agent-lab

`agent-lab` 是一个用 Rust 编写的 AI Agent 实验框架，当前以 **Tauri 桌面应用** 为主入口，同时保留可独立运行的 Agent 示例。目标是提供可组合的大模型 Agent、工具（Tools）、记忆（Memory）、RAG（检索增强生成）与多会话聊天能力。

> 当前仍处于实验阶段，部分接口为半成品或带有 `TODO`，适合用来学习、原型验证和扩展自己的 Agent 工作流。

## 特性

- **Tauri 桌面应用**：多会话聊天、流式响应、thinking/reasoning 展示、工具调用过程可视化
- **多种 Agent 实现**：`SimpleAgent`、`ReActAgent`、`ReflectionAgent`、`ToolAgent`、`RoundRobinGroupChat`
- **工具系统**：支持注册、调度、schema 生成与并发执行 tool calls
- **RAG 工具**：Markdown 分块、Embedding、HyDE + MQE 检索增强
- **记忆系统**：`working` / `episodic` / `semantic` / `perceptual` 四类记忆，底层组合 PostgreSQL + pgvector + Neo4j
- **OpenAI 兼容 LLM 客户端**：通过 `openai-api-rs` 支持 DeepSeek、OpenRouter、自建服务等
- **会话与消息持久化**：`chat_sessions` / `chat_messages` 表，支持多会话历史回溯

## 环境要求

- Rust（edition = `"2024"`）
- Node.js + pnpm（用于桌面端）
- PostgreSQL + `pgvector` 扩展
- Neo4j（用于记忆的实体-关系图）
- Ollama（默认运行 `nomic-embed-text`，维度 768）
- 任意 OpenAI 兼容的 LLM 服务
- SerpApi（仅使用 `web_search` 工具时需要）

> 如果你没有外部服务，也可以直接运行不依赖服务的库单元测试：`cargo test --lib`。

## 快速开始

1. **克隆仓库**

   ```bash
   git clone <repo-url>
   cd agent-lab
   ```

2. **准备外部服务**

   - 启动 PostgreSQL + pgvector
   - 启动 Neo4j
   - 启动 Ollama 并拉取 embedding 模型：

     ```bash
     ollama pull nomic-embed-text
     ```

3. **初始化数据库**

   ```bash
   psql $DATABASE_URL -f init_pg.sql
   ```

   该脚本会启用 `vector` 扩展，并创建 `memories`、`rag_chunks`、`chat_sessions`、`chat_messages` 表及索引。

4. **配置环境变量**

   在仓库根目录创建 `.env`（已加入 `.gitignore`，请勿提交）：

   ```bash
   # LLM（必填）
   API_KEY=your-api-key
   BASE_URL=https://api.deepseek.com/v1
   MODEL=deepseek-chat

   # 数据库（必填）
   DATABASE_URL=postgres://user:password@localhost/agent_lab

   # Neo4j
   NEO4J_URL=neo4j://127.0.0.1:7687
   NEO4J_USER=neo4j
   NEO4J_PASSWORD=your-neo4j-password

   # Embedding（可选，使用默认值即可）
   EMBEDDER_URL=http://localhost:11434/api/embeddings
   EMBEDDER_MODEL=nomic-embed-text

   # WebSearch（可选）
   SERPAPI_API_KEY=your-serpapi-key
   ```

5. **启动桌面端**

   ```bash
   pnpm install
   pnpm desktop
   ```

   即可启动 Tauri 桌面聊天应用。

## 运行示例

仓库提供了两个示例：

```bash
# 交互式 RAG Agent（终端）
pnpm lab
# 等价于：cargo run -p agent-lab-core --example rag_agent

# 多 Agent 小说创作
pnpm novel
# 等价于：cargo run -p agent-lab-core --example novel_generation
```

`rag_agent` 示例会读取 `API_KEY`、`BASE_URL`、`MODEL` 等环境变量；`novel_generation` 示例读取 `DEEPSEEK_API_KEY`、`DEEPSEEK_BASE_URL`、`DEEPSEEK_MODEL`。

## 测试

```bash
# 编译检查
cargo check

# 运行全部测试（部分需要 PG/Neo4j，否则会失败或跳过）
cargo test

# 只运行不依赖外部服务的库单元测试
cargo test --lib
```

## 项目结构

```text
Cargo.toml                   # Rust workspace 根
package.json                 # 任务聚合入口（pnpm lab / novel / desktop 等）
.cargo/config.toml           # cargo alias（lab / novel / c / t 等）
init_pg.sql                  # PG 建表/索引脚本
.crates/
│   ├── agent-lab-core/      # 核心库：Agent、工具、记忆、RAG、会话/消息服务
│   │   ├── src/
│   │   │   ├── lib.rs       # 库入口
│   │   │   ├── base/        # Agent、LLM、Config 基础抽象
│   │   │   ├── agent/       # Agent 实现
│   │   │   ├── tools/       # 工具系统（RAG、Memory、WebSearch 等）
│   │   │   ├── services/    # 业务服务（ChatService、SessionService、MessageService 等）
│   │   │   ├── storage/     # 存储层（PG、Neo4j、Embedder、ChatStore）
│   │   │   └── db.rs        # PostgreSQL 连接池
│   │   ├── examples/
│   │   │   ├── rag_agent.rs         # 终端交互式 RAG Agent
│   │   │   └── novel_generation.rs  # 多 Agent 小说创作
│   │   └── tests/
│   └── openai-api-rs/       # OpenAI 兼容客户端（workspace 成员）
.apps/
│   └── agent-lab-desktop/   # Tauri 桌面端（React + Tailwind/Windi CSS）
│       ├── src/             # 前端源码
│       └── src-tauri/       # Tauri Rust 后端（commands / services / state）
```

更详细的模块说明、测试策略和 AI 助手指南请参见 [`AGENTS.md`](./AGENTS.md)。

## 配置说明

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `API_KEY` | LLM API Key | - |
| `BASE_URL` | LLM 接口 base URL | - |
| `MODEL` | 模型名 | - |
| `PROVIDER` | 提供商标识 | `Custom` |
| `DATABASE_URL` | PostgreSQL 连接串 | - |
| `NEO4J_URL` | Neo4j Bolt URI | `neo4j://127.0.0.1:7687` |
| `NEO4J_USER` | Neo4j 用户名 | `neo4j` |
| `NEO4J_PASSWORD` | Neo4j 密码 | - |
| `EMBEDDER_URL` | Ollama embeddings 端点 | `http://localhost:11434/api/embeddings` |
| `EMBEDDER_MODEL` | Embedding 模型 | `nomic-embed-text` |
| `SERPAPI_API_KEY` | SerpApi 搜索 key | - |
| `DEFAULT_MODEL` / `DEFAULT_PROVIDER` | `Config` 默认值覆盖 | - |
| `TEMPERATURE` / `MAX_TOKENS` / `DEBUG` | `Config::from_env` 读取 | - |
| `DEEPSEEK_API_KEY` / `DEEPSEEK_BASE_URL` / `DEEPSEEK_MODEL` | 仅示例使用 | - |

## 注意事项

- `.env` 已加入 `.gitignore`，**不要将真实 API Key 或密码写入源码或提交到 git**。
- 当前 embedding 维度固定为 `768`（与 `init_pg.sql` 一致），更换 embedder 时需要同步修改表结构和代码中的 `dimension`。
- `crates/agent-lab-core/src/tools/base_shell/mod.rs` 中的 `BashShell` 当前被注释导出，启用后 Agent 将获得本地 shell 执行能力，请务必谨慎。
- 仓库根目录的 `node_modules/` 是历史遗留，不参与当前 Rust 构建流程。

## 许可证

（待补充，请根据项目实际情况填写）
