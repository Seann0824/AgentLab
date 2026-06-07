# DAG 系统与 Agent 核心集成状态分析

> **创建日期**: 2025-06-08
> **分析范围**: 代码库所有涉及 DAG 编排、Agent 执行、并行调度的模块
> **分析方法**: 逐个模块代码审查 + 依赖关系追踪

---

## 1. 核心问题

本分析回答两个关键问题：

1. **DAG 系统是否接入了完整的 Agent 主循环？**
2. **能否支持多个 Agent 任务并行执行？**

---

## 2. 系统全景图

```ascii
┌────────────────────────────────────────────────────────────────┐
│                     Agent 主循环 (agent.rs)                      │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Agent::run()  — 完整 ReAct 循环                          │  │
│  │  • LLM 调用 → 工具执行 → 结果反馈 → LLM 调用 → ...        │  │
│  │  • 上下文压缩 / 会话管理 / 任务状态管理                     │  │
│  │  • 支持的工具: edit, read, search, shell, spawn_agent,     │  │
│  │    pipeline_build/execute/list/status, debug, investigate  │  │
│  └──────────────────────────────────────────────────────────┘  │
│                               │                                  │
│                               ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              DAG 工具层 (tools/dag_tools/)                 │  │
│  │  pipeline_build → 解析 JSON → 注册 PipelineDef           │  │
│  │  pipeline_execute → 模拟执行（非真正 LLM 调用）           │  │
│  │  pipeline_list / pipeline_status → 查询状态               │  │
│  └──────────────────────────────────────────────────────────┘  │
│                               │                                  │
│                               ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                DAG 运行时层 (dag/)                          │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │  │
│  │  │  DAGEngine   │  │ NodeRuntime │  │ DataFlowManager │  │  │
│  │  │  (调度器)     │  │ (节点执行器) │  │ (数据流管理)     │  │  │
│  │  └─────────────┘  └─────────────┘  └─────────────────┘  │  │
│  └──────────────────────────────────────────────────────────┘  │
│                               │                                  │
│                               ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │          DAG 节点内部层 (dag/node_internal/)               │  │
│  │  ┌────────────────┐  ┌────────────────┐                  │  │
│  │  │  Worker Agent   │  │ Reviewer Agent │                  │  │
│  │  │  (单次 LLM 调用) │  │ (单次 LLM 调用) │                  │  │
│  │  └────────────────┘  └────────────────┘                  │  │
│  │         ↕                       ↕                         │  │
│  │  ┌──────────────────────────────────────────────────┐   │  │
│  │  │          NodeSupervisor (协调重试循环)            │   │  │
│  │  │  Worker → Reviewer → (通过→输出 / 不通过→重试)    │   │  │
│  │  └──────────────────────────────────────────────────┘   │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────┘
```

---

## 3. 集成状态逐层分析

### 3.1 主 Agent 循环 → DAG 工具层

**状态: ✅ 已连接**

主 Agent (`Agent::run()`) 可以调用 DAG 相关工具：

| 工具 | 功能 | 是否工作 | 说明 |
|------|------|---------|------|
| `pipeline_build` | 构建并注册 Pipeline | ✅ | 解析 JSON → 创建 PipelineDef → 验证 → 注册到全局 Store |
| `pipeline_execute` | 执行 Pipeline | ⚠️ **模拟执行** | 按拓扑顺序标记节点状态，**不调 LLM**，**不真正并行** |
| `pipeline_list` | 列出所有 Pipeline | ✅ | 查询 PipelineStore |
| `pipeline_status` | 查看执行状态 | ✅ | 查询 EngineStore / PipelineStore |

**关键代码位置**:
- `src/tools/dag_tools/` — 所有 DAG 工具的实现
- `src/tools/mod.rs` — DAG 工具的注册入口

### 3.2 DAGEngine 调度器 → NodeRuntime

**状态: ⚠️ 调度逻辑已设计，但实际执行未接入**

`DAGEngine` 的调度设计是完整的：

```rust
// engine.rs 第 68-86 行 — 设计良好的并行调度
pub fn ready_nodes(&self) -> Vec<String> {
    let max_concurrency = self.pipeline.config.max_concurrency;
    // 计算当前可并行执行的节点（基于入度和 max_concurrency）
}
```

但 `pipeline_execute` 工具的 `execute_inner()` 方法（`execute.rs` 第 68-96 行）**只是模拟**：

```rust
for node_id in &execution_order {
    // 顺序遍历，逐个标记 Working → Completed
    instance.transition_to(NodeStatus::Working);
    instance.transition_to(NodeStatus::Approved);
}
```

**没有被调用的真正执行路径**:
- `NodeRuntime::execute_node()` （`runtime.rs`） — 实际会调 LLM 的执行路径，**从未被 `pipeline_execute` 调用**
- `DAGEngine` 中的 `ready_nodes()` 并行逻辑 — 设计存在但无执行器驱动

### 3.3 NodeRuntime → NodeSupervisor → Worker/Reviewer Agent

**状态: ✅ 节点内部链路完整**（但仅从代码可直达，非工具调用路径）

`NodeRuntime::execute_node()` → `NodeSupervisor::execute_with_retry()` → `WorkerAgent::execute()` + `ReviewerAgent::review()`

这条链路如果手动调用可以正常工作，但**没有被任何 DAG 工具触发**。

### 3.4 Worker/Reviewer Agent → LLM

**状态: ✅ 独立工作，但缺少工具能力**

Worker 和 Reviewer 都通过 `call_llm()` 直接调用 LLM：

```rust
// runtime.rs 第 44-77 行 — 简化版 LLM 调用
pub async fn call_llm(model, messages, tools) -> DAGResult<(String, Value)> {
    let mut stream = model.lock().await.stream_chat(&messages, tools);
    // ... 收集响应文本
}
```

**对比**：主 Agent 循环的 LLM 调用包含完整的 ReAct 循环（思考→工具调用→观察→思考...），而 DAG 内部的 Worker/Reviewer 只是**单次 LLM 调用**。

**Worker Agent 不支持工具调用**（`runtime.rs` 第 69-71 行）：
```rust
ModelEvent::ToolCallBlock { .. } => {
    // 简版实现：不支持工具调用
    // Phase 2 增强版可添加工具循环
}
```

---

## 4. 并行能力评估

### 4.1 主 Agent 实例级并行

**状态: ✅ 框架完整，未与 DAG 集成**

`agent.rs` 提供了完整的并行基础设施：

| 能力 | 位置 | 说明 |
|------|------|------|
| `AgentHandle` | `agent.rs` 第 74-91 行 | 基于 `tokio::task::JoinHandle` 的并发句柄 |
| `Agent::run()` | `agent.rs` 第 ~300 行 | 完整的 ReAct 主循环 |
| `Agent::run_parallel()` | `agent.rs` 第 ~400 行 | 通过 `FuturesUnordered` 并发运行多个 Agent |
| 独立 Session/Context | `SessionManager` | 每个 Agent 有独立会话和上下文 |

**典型并行模式**（当前可用）：
```rust
let agent_a = Agent::builder().model(model_a).build()?;
let agent_b = Agent::builder().model(model_b).build()?;
let (result_a, result_b) = tokio::join!(agent_a.run(), agent_b.run());  // 对等并行
```

**未被 DAG 使用**：DAG 节点的 Worker 没有走 `Agent::run()` 路径，因此无法利用此并行机制。

### 4.2 DAG 节点间并行（拓扑无关节点）

**状态: ⚠️ 设计存在，执行层未实现**

| 层面 | 状态 | 说明 |
|------|------|------|
| **数据结构** | ✅ | `PipelineConfig.max_concurrency` 已定义，默认 2 |
| **就绪节点计算** | ✅ | `DAGEngine::ready_nodes()` 正确实现 |
| **状态追踪** | ✅ | `NodeStatus` 完整的状态机（Pending→Ready→Working→Reviewing→Approved→Completed） |
| **实际并行执行** | ❌ | `pipeline_execute` 工具仅模拟，顺序执行 |
| **异步调度器** | ❌ | 没有 tokio::spawn 驱动的节点执行轮询循环 |

**对比设计文档 vs 代码实现**：

设计文档 (`dag-task-orchestration.md`) 承诺的并行执行：
```
Example: 4 个节点，A→C, B→C, C→D
时间线:
  T1: A 和 B 并行执行（无依赖）
  T2: A 和 B 完成后，C 开始执行
  T3: C 完成后，D 开始执行
```

代码中实际能做到的：`pipeline_execute` 只是 `for node_id in &execution_order { mark_completed() }`。

### 4.3 节点内部并行（Worker + Reviewer）

**状态: ❌ 串行设计**

每个节点内部是 Worker → Reviewer 的**串行顺序**：

```rust
// supervisor.rs — 串行流程
let worker_output = WorkerAgent::execute(ctx, worker_config).await?;   // 先 Worker
let review = ReviewerAgent::review(ctx, reviewer_config).await?;       // 再 Reviewer
```

这是设计使然（审核依赖 Worker 的输出），不构成问题。

---

## 5. 集成差距总结

| 集成点 | 当前状态 | 目标状态 | 差距 |
|--------|---------|---------|------|
| Agent 主循环调用 DAG 工具 | ✅ 已连接 | ✅ 维持 | 无 |
| DAG 节点走完整 Agent 循环 | ❌ 单次 LLM 调用 | ✅ 完整 ReAct + 工具 | **大** |
| DAG 节点支持工具调用 | ❌ 硬编码跳过 | ✅ 使用 ToolManager | **大** |
| 节点间真正并行执行 | ❌ 模拟执行 | ✅ 异步并行调度 | **大** |
| DAG 使用主 Agent 并行框架 | ❌ 未连接 | ✅ 节点利用 Agent::run() | **大** |
| 节点超时控制 | ⚠️ 配置存在 | ✅ 异步超时 | **中** |
| 执行结果持久化 | ⚠️ Store 保存 | ✅ Pipeline 状态持久化 | **小** |

### 5.1 关键代码行级证据

```
src/tools/dag_tools/execute.rs:12
  // 目前模拟 DAGEngine 的调度执行过程（在 Phase 4 中将集成真正的 Worker/Reviewer LLM 调用）

src/dag/runtime.rs:69-71
  ModelEvent::ToolCallBlock { .. } => {
      // 简版实现：不支持工具调用

src/dag/engine.rs:68-86
  fn ready_nodes(&self) -> Vec<String> { ... }  // 设计存在但无人调用

src/agent.rs 中 DAGContext 与 Agent 无关联
```

---

## 6. 架构关系图：三层 Agent 模型

```ascii
┌────────────────────────────────────────────────────────────────────┐
│  第一层：主 Agent（完整 ReAct）                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ Agent::run()                                                 │  │
│  │ 用户输入 → LLM(思考+工具选择) → 工具执行 → 观察 → LLM → ...  │  │
│  │ 工具集: edit, read, search, shell, spawn_agent,              │  │
│  │         pipeline_build/execute/list/status, debug            │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                              ▼                                       │
│  第二层：DAG 编排层                                                │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ DAGEngine 调度器 + PipelineDef/PipelineConfig                │  │
│  │ max_concurrency=2, node_timeout_seconds=300                   │  │
│  │ 拓扑排序, 状态机管理, 数据路由                                │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                              ▼                                       │
│  第三层：节点内部 Agent（简化版）                                  │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ WorkerAgent::execute()   → 单次 LLM 调用 → 无工具能力         │  │
│  │ ReviewerAgent::review()  → 单次 LLM 调用 → 无工具能力         │  │
│  │ NodeSupervisor           → Worker↔Reviewer 重试循环           │  │
│  └──────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────┘
```

**问题**：第三层没有复用第一层的 Agent 循环基础设施，导致：
- DAG 节点 Agent 没有工具调用能力
- DAG 节点 Agent 没有上下文管理
- DAG 节点 Agent 无法利用主 Agent 的并行框架

---

## 7. 打通路线图

### Phase A：让 DAG 节点接入完整 Agent 循环（优先级最高）

**目标**：DAG 节点的 Worker 使用 `Agent::run()` 而不是 `WorkerAgent::execute()`。

**变更**：
1. 改造 `DAGContext` 使其包含共享的 `Agent` 或 `ToolManager` 引用
2. `WorkerAgent::execute()` 改为启动一个带工具能力的 Agent 子循环
3. `call_llm()` 扩展为支持工具调用（去掉 `ToolCallBlock` 的 skip）

```rust
// 改造后伪代码
impl WorkerAgent {
    async fn execute(ctx: &DAGContext, config: WorkerConfig) -> DAGResult<WorkerOutput> {
        let agent = Agent::builder()
            .model(ctx.model.clone())
            .tool_manager(ctx.tool_manager.clone())
            .task(Task::new(config.instruction, config.input))
            .build()?;
        let result = agent.run().await?;  // ← 走完整 ReAct 循环
        Ok(WorkerOutput { content: result.output, ... })
    }
}
```

### Phase B：实现真正的并行调度（次优先级）

**目标**：`pipeline_execute` 实际使用 `DAGEngine::ready_nodes()` 并真正并发执行节点。

**变更**：
1. 重写 `pipeline_execute` 工具，使用 tokio 异步调度
2. 实现节点执行循环（轮询 ready_nodes → spawn 执行 → 处理完成 → 继续轮询）
3. 加入节点超时控制（`node_timeout_seconds`）

```rust
// 并行调度伪代码
async fn execute_pipeline_real(engine: &mut DAGEngine, ctx: &DAGContext) {
    loop {
        let ready = engine.ready_nodes();  // 基于 max_concurrency 和依赖关系
        if ready.is_empty() && engine.all_terminal() { break; }
        
        let handles: Vec<_> = ready.iter().map(|node_id| {
            tokio::spawn(execute_node(ctx, node_def, input))
        }).collect();
        
        for handle in handles {
            let (node_id, result) = handle.await?;
            engine.on_node_completed(&node_id, result);
        }
    }
}
```

### Phase C：深度集成（长期）

1. **DAG 节点使用主 Agent 并行框架** — 节点复用 `Agent::run_parallel()` 而非手写并行逻辑
2. **每个节点可配置不同模型** — `PipelineConfig.worker_model` / `reviewer_model` 实际生效
3. **子 Pipeline 嵌套** — 一个节点可以是一个子 Pipeline（递归 DAG）
4. **DAG 事件持久化 + 断点续跑** — 利用 `persistence.rs` 和 `event_bus.rs`

---

## 8. 当前可执行的并行能力（即便不改造）

即便 DAG 系统尚未完全打通，**当前系统已经具备以下并行能力**：

### 8.1 手动多 Pipeline 并行

你（主 Agent）可以同时发起多个互不依赖的 Pipeline 执行（通过 spawn_agent 或其他手段），因为它们各自独立注册和执行。

### 8.2 主 Agent + SubAgent 并行

```rust
// 主 Agent 运行自己的任务
// 同时 spawn_agent 派生子 Agent 执行独立任务
// 两者并行不悖
```

### 8.3 对等多 Agent 并行（通过 AgentBuilder）

```rust
// main.rs 中可以启动多个 Agent 并行
let h1 = Agent::spawn("agent-1", config1, model1, tools1);
let h2 = Agent::spawn("agent-2", config2, model2, tools2);
// 各自独立运行
```

但这些都不是**DAG 编排的并行**——它们是独立的任务，没有 DAG 的依赖协调和数据路由。

---

## 9. 推荐行动方案

| 优先级 | 任务 | 预期效果 | 工作量估算 |
|--------|------|---------|-----------|
| 🔴 P0 | Worker Agent 接入完整 Agent 循环 | DAG 节点获得工具调用能力 | 3-5 天 |
| 🔴 P0 | call_llm 支持工具调用 | Worker/Reviewer 可使用工具 | 1-2 天 |
| 🟡 P1 | pipeline_execute 改为真正并行执行 | DAG 多节点真正并行 | 3-5 天 |
| 🟡 P1 | DAGContext 与 Agent 核心共享资源 | 减少重复代码，统一资源管理 | 2-3 天 |
| 🟢 P2 | PipelineConfig 模型配置生效 | 节点可指定不同 LLM 模型 | 1 天 |
| 🟢 P2 | 节点超时控制实现 | 避免死节点阻塞 Pipeline | 1-2 天 |
| 🔵 P3 | 子 Pipeline 嵌套 | 支持递归 DAG | 5-7 天 |
| 🔵 P3 | 断点续跑 | Pipeline 执行可中断恢复 | 3-5 天 |

---

## 10. 附录：关键文件索引

| 文件 | 行数 | 核心职责 | 与集成的关系 |
|------|------|---------|------------|
| `src/agent.rs` | 1011 | 主 Agent 循环 + 并行框架 | 需要被 DAG 节点复用 |
| `src/dag/engine.rs` | 328 | DAG 调度器 + 状态管理 | 调度逻辑已具备，缺执行器 |
| `src/dag/runtime.rs` | 111 | 节点执行器 + LLM 调用 | `call_llm` 需要扩展工具支持 |
| `src/dag/node_internal/worker.rs` | 93 | Worker Agent | 需改为使用 Agent::run() |
| `src/dag/node_internal/reviewer.rs` | 189 | Reviewer Agent | 需支持工具（可选） |
| `src/dag/node_internal/supervisor.rs` | 140 | Worker+Reviewer 协调 | 重试逻辑保留不变 |
| `src/tools/dag_tools/execute.rs` | 112 | pipeline_execute 工具 | 需重写为真正执行 |
| `src/dag/pipeline.rs` | 226 | Pipeline 定义 | `config` 字段已预留并行配置 |
| `src/dag/dataflow.rs` | 131 | 数据路由与合并 | 已实现，可直接使用 |
| `src/dag/persistence.rs` | — | 持久化 | 待集成 |
| `src/dag/event_bus.rs` | — | 事件总线 | 待集成 |

---

> **核心结论**：DAG 系统的**架构设计是好的**——分层清晰、状态机完整、并行调度逻辑设计到位。但当前处于"定义层和工具层已完成、运行时层和节点执行层未真正打通"的阶段。让 DAG 节点接入完整的 Agent 主循环（带工具调用的 ReAct 循环）是实现真正并行多 Agent 执行的关键前置条件。
