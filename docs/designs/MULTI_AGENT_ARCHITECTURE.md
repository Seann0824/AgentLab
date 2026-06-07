# 多 Agent 架构设计方案

> **创建日期**: 2025-06-08
> **状态**: ✅ 已实现

---

## 1. 设计目标

### 1.1 核心目标
1. **提取主循环** — 将 `main.rs` 中 700+ 行的主循环逻辑提取到 `src/agent.rs`，形成可复用的 `Agent` 结构体
2. **支持多 Agent** — 能够创建、管理、通信多个 Agent 实例
3. **保持兼容** — 现有功能（CLI 交互、`--task` 模式、会话管理、上下文压缩）不受影响
4. **可配置** — Agent 配置化，支持 `AgentConfig`

### 1.2 非目标
- 不引入分布式 agent 通信协议
- 不改变现有工具系统
- 不改变上下文管理逻辑

---

## 2. 架构设计

### 2.1 核心类型

```rust
/// Agent 配置
pub struct AgentConfig {
    pub token_limit: usize,          // 上下文 Token 上限（默认 128000）
    pub max_turns: usize,            // 最大轮次（默认 20）
    pub trigger_ratio: f64,          // 压缩触发比例（默认 0.7）
    pub enable_async_summary: bool,  // 是否启用异步摘要
    pub enable_tool_pruning: bool,   // 是否启用工具调用修剪
    pub tool_pruning_keep_recent: usize,     // 保留最近工具调用数
    pub tool_pruning_max_output_chars: usize, // 工具输出最大字符
}

/// Agent — 持有所有状态，运行主循环
pub struct Agent {
    config: AgentConfig,
    model: Box<dyn ModelAdapter>,
    tool_manager: ToolManager,
    context_manager: ContextManager,
    task_manager: TaskManager,
    session_manager: SessionManager,
    command_registry: CommandRegistry,
    current_dir: String,
}

/// AgentHandle — 多 Agent 运行的句柄
pub struct AgentHandle {
    pub name: String,
    pub task: tokio::task::JoinHandle<anyhow::Result<()>>,
}
```

### 2.2 模块依赖关系

```
main.rs (thin)
  └── Agent::run()         ← src/agent.rs
        ├── ContextManager  ← src/context/
        ├── ToolManager     ← src/tools/
        ├── TaskManager     ← src/task/
        ├── SessionManager  ← src/session/
        ├── CommandRegistry ← src/cli/
        └── ModelAdapter    ← src/model/
```

### 2.3 多 Agent 运行模型

```
AgentLab (协调器)
  ├── Agent "default"    ← 主交互 Agent
  │     └── AgentHandle (JoinHandle)
  ├── Agent "worker-1"   ← 后台工作 Agent（可选）
  │     └── AgentHandle (JoinHandle)
  └── Agent "worker-2"   ← 后台工作 Agent（可选）
        └── AgentHandle (JoinHandle)
```

每个 Agent 运行在自己的 `tokio::task::spawn` 中：
- 独立上下文管理器
- 独立工具管理器（可共享或独立）
- 独立会话管理器

---

## 3. 关键设计决策

### 3.1 Agent 构建器模式

使用 `AgentBuilder` 提供链式构造：

```rust
let agent = Agent::builder()
    .model(my_model)
    .tool_manager(my_tools)
    .config(AgentConfig::default())
    .current_dir("/path")
    .build()?;

agent.run().await?;
```

### 3.2 多 Agent 启动

```rust
// 方式1：直接运行（当前 main.rs 行为）
Agent::run_default().await?;

// 方式2：构建并运行
let handle = Agent::spawn("worker", config, model, tools).await?;
handle.await??;

// 方式3：多个 agent 并行
let handles = vec![
    Agent::spawn("agent-a", config_a, model_a, tools_a),
    Agent::spawn("agent-b", config_b, model_b, tools_b),
];
for h in handles {
    h.await??;
}
```

### 3.3 向后兼容

- `Agent::run()` 保留 `main.rs` 的所有功能（/命令、会话管理、--task 模式等）
- `AgentConfig` 默认值与当前 `main.rs` 一致
- 新增的 `agent` 模块通过 `lib.rs` 导出

---

## 4. 文件变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/agent.rs` | 重写 | 从空文件 → Agent + AgentConfig + AgentBuilder + AgentHandle |
| `src/lib.rs` | 修改 | 添加 `pub mod agent;` |
| `src/main.rs` | 大幅精简 | 只保留 CLI 参数解析和 Agent 创建 |

### 4.1 agent.rs 包含的公共 API

```rust
// 模块导出
pub use agent::Agent;
pub use config::AgentConfig;

// AgentBuilder（构建器）
pub struct AgentBuilder { ... }
impl AgentBuilder {
    pub fn new() -> Self;
    pub fn model(mut self, model: Box<dyn ModelAdapter>) -> Self;
    pub fn tool_manager(mut self, tm: ToolManager) -> Self;
    pub fn config(mut self, config: AgentConfig) -> Self;
    pub fn current_dir(mut self, dir: impl Into<String>) -> Self;
    pub fn build(self) -> anyhow::Result<Agent>;
}

// Agent（核心）
pub struct Agent { ... }
impl Agent {
    pub fn builder() -> AgentBuilder;
    pub async fn run(&mut self) -> anyhow::Result<()>;
    pub fn spawn(name: &str, agent: Agent) -> AgentHandle;
}

// AgentHandle（多 Agent 句柄）
pub struct AgentHandle { ... }
impl AgentHandle {
    pub fn name(&self) -> &str;
    pub async fn join(self) -> anyhow::Result<()>;
}
```

---

## 5. 验证标准

1. ✅ `cargo check` 编译通过
2. ✅ `cargo test` 全部测试通过
3. ✅ `--task` 模式正常（子 agent 单次运行）
4. ✅ 普通交互模式正常（stdin 读取、/ 命令、会话管理）
5. ✅ `Agent::spawn()` 可创建独立运行的子 agent
