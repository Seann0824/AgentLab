# DAG 任务编排系统设计方案

> **创建日期**: 2025-06-08
> **状态**: 📝 设计中
> **版本**: v0.1

---

## 1. 设计目标

### 1.1 核心目标

构建一个基于**有向无环图（DAG）** 的任务编排系统，使得：

1. **复杂任务可拆解** — 将一个大型任务拆解为多个相互依赖的子任务节点，形成 DAG
2. **节点内双 Agent 协作** — 每个任务节点内部由一个**工作 Agent**（Worker）和一个**审核 Agent**（Reviewer）组成
3. **审核通过自动转发** — 节点输出通过审核后，自动转发到下游依赖节点
4. **并行执行** — 无相互依赖的节点可以并行执行，提高效率
5. **可观测性** — 每个节点的执行状态、审核结果、传输数据都可追踪

### 1.2 非目标

- 不引入分布式 Agent 通信协议（仍使用进程内或子进程模式）
- 不改变现有 Agent 核心主循环
- 不改变现有 Tool 系统接口
- 不引入外部工作流引擎

---

## 2. 整体架构

### 2.1 分层架构

```
┌──────────────────────────────────────────────────────────────┐
│                    DAG 定义层 (Definition)                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ PipelineDef  │  │   NodeDef   │  │   EdgeDef (依赖关系) │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├──────────────────────────────────────────────────────────────┤
│                    DAG 运行时层 (Runtime)                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  DAGEngine   │  │ NodeRuntime │  │   DataFlowManager   │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├──────────────────────────────────────────────────────────────┤
│                   节点内部层 (Node Internal)                   │
│  ┌──────────────────┐  ┌──────────────────┐                 │
│  │   Worker Agent    │  │  Reviewer Agent   │                 │
│  │  (执行工作任务)    │  │  (审核工作结果)    │                 │
│  └──────────────────┘  └──────────────────┘                 │
├──────────────────────────────────────────────────────────────┤
│                    基础设施层 (Infrastructure)                │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │    Agent     │  │  ToolManager │  │   ContextManager    │  │
│  │  (现有核心)   │  │  (现有工具)   │  │  (现有上下文管理)    │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└───────────────────────────────────────────��──────────────────┘
```

### 2.2 模块依赖关系

```
src/
├── dag/                          ← 新增：DAG 编排系统
│   ├── mod.rs                    ← 模块入口，导出所有公共类型
│   ├── pipeline.rs               ← PipelineDef 定义
│   ├── node.rs                   ← NodeDef / NodeInstance 定义
│   ├── edge.rs                   ← 边定义（依赖关系 + 数据映射）
│   ├── engine.rs                 ← DAGEngine — 核心调度器
│   ├── runtime.rs                ← NodeRuntime — 节点执行器
│   ├── dataflow.rs               ← 数据流管理（输入/输出传递）
│   └── types.rs                  ← 状态枚举、事件类型等
│
├── dag/node_internal/            ← 新增：节点内部双 Agent 实现
│   ├── mod.rs                    ← 模块入口
│   ├── worker.rs                 ← Worker Agent 封装
│   ├── reviewer.rs               ← Reviewer Agent 封装
│   └── supervisor.rs             ← 节点内部协调器
│
├── agent.rs                      ← 现有：Agent 核心（扩展 AgentBuilder）
├── tools/                        ← 现有：工具系统
│   └── dag_tools/                ← 新增：DAG 相关工具
│       ├── mod.rs                ← pipeline_build / dag_execute 等工具
│       └── ...
└── lib.rs                        ← 修改：添加 pub mod dag;
```

---

## 3. 核心数据模型

### 3.1 DAG Pipeline 定义

```rust
/// Pipeline — 一个完整的 DAG 任务定义
pub struct PipelineDef {
    /// 唯一标识
    pub id: String,
    /// 描述
    pub description: String,
    /// 所有节点定义
    pub nodes: Vec<NodeDef>,
    /// 所有边定义（依赖关系）
    pub edges: Vec<EdgeDef>,
    /// DAG 全局配置
    pub config: PipelineConfig,
}

/// Pipeline 全局配置
pub struct PipelineConfig {
    /// 最大并行节点数
    pub max_concurrency: usize,
    /// 节点超时秒数
    pub node_timeout_seconds: u64,
    /// 审核最大重试次数
    pub max_review_retries: u32,
    /// 是否在审核失败时跳过节点（标记为 skipped）
    pub skip_on_review_fail: bool,
    /// 工作 Agent 的模型（不指定则使用全局模型）
    pub worker_model: Option<String>,
    /// 审核 Agent 的模型（不指定则使用全局模型）
    pub reviewer_model: Option<String>,
}
```

### 3.2 节点定义

```rust
/// NodeDef — 节点定义（DAG 中的一个任务节点）
pub struct NodeDef {
    /// 节点唯一标识
    pub id: String,
    /// 节点名称
    pub name: String,
    /// 节点描述（作为 Worker 的系统提示）
    pub description: String,
    /// Worker 的详细指令
    pub worker_instruction: String,
    /// Reviewer 的审核标准
    pub review_criteria: ReviewCriteria,
    /// 输入模式
    pub input_mode: InputMode,
    /// 输出模式
    pub output_mode: OutputMode,
    /// 节点标签（用于分类和过滤）
    pub tags: Vec<String>,
}

/// 审核标准
pub struct ReviewCriteria {
    /// 审核清单（逐条检查）
    pub check_items: Vec<String>,
    /// 审核指南
    pub guidelines: String,
}

/// 输入模式
pub enum InputMode {
    /// 接收所有上游输出合并后的数据
    Merged,
    /// 选择特定上游字段
    Select { from_node: String, fields: Vec<String> },
    /// 接收原始用户输入
    RawInput,
}

/// 输出模式
pub enum OutputMode {
    /// 原始文本输出
    Text,
    /// 结构化 JSON 输出
    Json { schema: Option<serde_json::Value> },
    /// 文件输出
    File { path_pattern: String },
}
```

### 3.3 边定义

```rust
/// EdgeDef — 边的定义（节点间的依赖和数据流向）
pub struct EdgeDef {
    /// 来源节点 ID
    pub from: String,
    /// 目标节点 ID
    pub to: String,
    /// 数据映射规则（可选）
    pub data_mapping: Option<DataMapping>,
}

/// 数据映射规则
pub struct DataMapping {
    /// 从源输出的字段提取
    pub source_fields: Vec<String>,
    /// 映射到目标输入的字段名
    pub target_fields: Vec<String>,
    /// 数据转换表达式（可选，预留）
    pub transform: Option<String>,
}
```

### 3.4 运行时状态

```rust
/// 节点运行时状态
#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    /// 等待依赖就绪
    Pending,
    /// 依赖已就绪，等待调度
    Ready,
    /// Worker 正在执行
    Working,
    /// Reviewer 正在审核
    Reviewing,
    /// 审核通过
    Approved,
    /// 审核不通过，需要重试
    Rejected { retry_count: u32, reason: String },
    /// 已完成（审核通过且输出已转发）
    Completed,
    /// 执行失败（不可恢复错误）
    Failed { error: String },
    /// 跳过（配置为失败时跳过）
    Skipped { reason: String },
}

/// 节点运行时实例
pub struct NodeInstance {
    /// 对应 NodeDef.id
    pub node_id: String,
    /// 当前状态
    pub status: NodeStatus,
    /// 接收到的输入数据
    pub input: Option<serde_json::Value>,
    /// Worker 产生的输出
    pub worker_output: Option<serde_json::Value>,
    /// 审核结果
    pub review_result: Option<ReviewResult>,
    /// 最终输出（审核通过后的转发数据）
    pub final_output: Option<serde_json::Value>,
    /// 执行日志
    pub logs: Vec<NodeLog>,
    /// 开始时间
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 完成时间
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 审核结果
pub struct ReviewResult {
    pub passed: bool,
    pub score: Option<f32>,
    pub feedback: String,
    pub details: Vec<CheckResult>,
}

pub struct CheckResult {
    pub item: String,
    pub passed: bool,
    pub comment: String,
}
```

---

## 4. 核心引擎设计

### 4.1 DAGEngine — 图调度器

DAGEngine 是整个系统的核心，负责：

1. **拓扑排序** — 基于 `edges` 计算节点的执行顺序，检测环
2. **状态管理** — 维护所有 `NodeInstance` 的状态
3. **并行调度** — 当多个节点的依赖全部就绪时，并行派发
4. **数据路由** — 节点完成后，将 `final_output` 路由到下游节点

```
DAGEngine 主循环（简化）:

loop {
    // Step 1: 找出所有 Ready 的节点
    let ready_nodes = find_ready_nodes(&instances, &edges);

    if ready_nodes.is_empty() && all_completed(&instances) {
        break; // 所有节点已完成
    }

    // Step 2: 并行执行 Ready 节点（受 max_concurrency 限制）
    for node in ready_nodes {
        spawn_node_execution(node);
    }

    // Step 3: 等待任一节点完成
    let completed = wait_for_any_completion().await;

    // Step 4: 更新状态，转发数据
    for each downstream_node of completed {
        update_input(downstream_node, completed.final_output);
    }

    // Step 5: 重新计算 Ready 节点
}
```

### 4.2 拓扑排序与环检测

```rust
impl DAGEngine {
    /// 拓扑排序，如果检测到环则返回错误
    pub fn topological_sort(&self) -> Result<Vec<String>, DAGError> {
        // 使用 Kahn 算法：
        // 1. 计算每个节点的入度
        // 2. 将入度为 0 的节点加入队列
        // 3. 依次出队，减少下游入度
        // 4. 如果最终处理节点数 < 总节点数，说明存在环
    }
}
```

### 4.3 节点执行流程

```
┌───────────────────────────────────────────��────────────┐
│                  节点执行流程                             │
│                                                         │
│  Pending                                                 │
│    │                                                     │
│    ▼                                                     │
│  Ready ←── 所有上游依赖已完成                            │
│    │                                                     │
│    ▼                                                     │
│  Working ←── Worker Agent 开始执行                       │
│    │                                                     │
│    ├── Worker 完成任务，产生 output                       │
│    │                                                     │
│    ▼                                                     │
│  Reviewing ←── Reviewer Agent 审核 output               │
│    │                                                     │
│    ├── 审核通过?                                         │
│    │   ├── Yes ──▶ Approved ──▶ Completed                │
│    │   └── No  ──▶ Rejected                              │
│    │               │                                     │
│    │               ├── retry_count < max?                 │
│    │               │   ├── Yes ──▶ Working (重试)         │
│    │               │   └── No  ──▶ Failed / Skipped      │
│    │                                                     │
│    ▼                                                     │
│  Completed ──▶ 转发 final_output 到下游节点               │
│                                                         │
└────────────────────────────────────────────────────────┘
```

---

## 5. 节点内部双 Agent 设计

### 5.1 节点内部架构

```
┌──────────────────────────────────────┐
│           Node Supervisor              │
│  (协调 Worker + Reviewer 的生命周期)   │
│                                       │
│  ┌─────────────────┐                  │
│  │  Worker Agent    │                  │
│  │                   │                  │
│  │  - 接收输入数据   │                  │
│  │  - 执行工作任务   │                  │
│  │  - 产生原始输出   │                  │
│  └────────┬────────┘                  │
│           │ output                    │
│           ▼                           │
│  ┌─────────────────┐                  │
│  │  Reviewer Agent  │                  │
│  │                   │                  │
│  │  - 接收原始输出   │                  │
│  │  - 按标准逐项审核  │                  │
│  │  - 输出审核结果   │                  │
│  └────────┬────────┘                  │
│           │ passed/failed             │
│           ▼                           │
│   决策: 通过 → 转发输出               │
│         不通过 → 给 Worker 反馈重试    │
└──────────────────────────────────────┘
```

### 5.2 Worker Agent

Worker Agent 是一个**独立运行的 Agent 实例**，拥有自己的上下文和工具集。

```rust
/// Worker Agent 配置
pub struct WorkerConfig {
    /// Agent 名称
    pub name: String,
    /// 任务描述（作为系统提示）
    pub instruction: String,
    /// 输入数据（由上游提供）
    pub input: serde_json::Value,
    /// 可用的工具列表（可限制 Worker 能使用的工具）
    pub allowed_tools: Vec<String>,
    /// 模型配置（可选，不指定使用全局默认）
    pub model_config: Option<ModelConfig>,
    /// 最大执行轮次
    pub max_turns: usize,
}

/// Worker 执行结果
pub struct WorkerOutput {
    /// 原始输出内容
    pub content: String,
    /// 结构化输出（如果有定义 schema）
    pub structured: Option<serde_json::Value>,
    /// 执行日志
    pub execution_log: Vec<String>,
    /// 耗时
    pub duration: std::time::Duration,
}
```

Worker 的执行流程：
1. 接收 `WorkerConfig`，构建一个**隔离的 Agent 实例**
2. 设置系统提示（包括节点描述、输入数据、输出要求）
3. 运行 Agent 主循环，直到产生最终输出或达到 `max_turns`
4. 输出格式化为 `WorkerOutput`

### 5.3 Reviewer Agent

Reviewer Agent 是**另一个独立的 Agent 实例**，专注于审核 Worker 的输出。

```rust
/// Reviewer Agent 配置
pub struct ReviewerConfig {
    /// Agent 名称
    pub name: String,
    /// 审核标准
    pub criteria: ReviewCriteria,
    /// Worker 的原始输出
    pub worker_output: WorkerOutput,
    /// 原始输入（用于上下文对照）
    pub original_input: serde_json::Value,
    /// 审核模式
    pub mode: ReviewMode,
}

pub enum ReviewMode {
    /// 逐项检查（按 check_items 列表逐一审核）
    Checklist,
    /// 自由评估（给出整体评分和反馈）
    FreeForm,
    /// 对比审核（与预期结果对比）
    Comparison { expected: serde_json::Value },
}

/// 审核结果
pub struct ReviewOutput {
    pub passed: bool,
    pub score: f32,
    pub feedback: String,
    pub check_results: Vec<CheckResult>,
    pub suggestions: Vec<String>,
}
```

Reviewer 的执行流程：
1. 接收 `ReviewerConfig`，构建一个**独立的 Agent 实例**
2. 设置审核系统提示（包括审核标准、Worker 输出、原始输入）
3. 运行 Agent 主循环，逐项检查或自由评估
4. 输出结构化的审核结果 `ReviewOutput`

### 5.4 Node Supervisor — 内部协调器

Node Supervisor 管理节点内部的生命周期：

```rust
pub struct NodeSupervisor {
    config: NodeDef,
    worker: Option<WorkerAgent>,
    reviewer: Option<ReviewerAgent>,
}

impl NodeSupervisor {
    /// 执行节点（输入 → Worker → Reviewer → 输出/反馈）
    pub async fn execute(
        &mut self,
        input: serde_json::Value,
    ) -> Result<NodeResult, NodeError> {
        // Phase 1: Worker 执行
        let worker_output = self.run_worker(&input).await?;

        // Phase 2: Reviewer 审核
        let review_result = self.run_reviewer(&worker_output, &input).await?;

        if review_result.passed {
            Ok(NodeResult::Success {
                output: worker_output.content,
                review: review_result,
            })
        } else {
            Ok(NodeResult::NeedsRevision {
                worker_output,
                review: review_result,
            })
        }
    }

    /// 带重试的执行（Worker + Reviewer 循环直到通过或耗尽重试次数）
    pub async fn execute_with_retry(
        &mut self,
        input: serde_json::Value,
        max_retries: u32,
    ) -> Result<NodeResult, NodeError> {
        let mut retries = 0;
        loop {
            let result = self.execute(input.clone()).await?;
            match result {
                NodeResult::Success { .. } => return Ok(result),
                NodeResult::NeedsRevision { worker_output, review } => {
                    if retries >= max_retries {
                        return Ok(NodeResult::FailedAfterRetries {
                            last_worker_output: worker_output,
                            last_review: review,
                            retries,
                        });
                    }
                    // 将审核反馈作为新的上下文注入 Worker，重新执行
                    retries += 1;
                }
            }
        }
    }
}
```

---

## 6. 数据流设计

### 6.1 数据传递模型

```
                    ┌──────────┐
                    │  Node A   │
                    └────┬─────┘
                         │ output: { "summary": "...", "data": {...} }
                         │
           ┌─────────────┼─────────────┐
           │             │             │
           ▼             ▼             ▼
    ┌──────────┐  ┌──────────┐  ┌──────────┐
    │  Node B   │  │  Node C   │  │  Node D   │
    └──────────┘  └──────────┘  └──────────┘
         │                        │
         │                        │
         ▼                        ▼
    ┌─────────────────────────────────┐
    │            Node E                │
    │  (接收 B 和 D 的合并输出)         │
    └─────────────────────────────────┘
```

### 6.2 数据合并策略

当下游节点有多个上游时，需要将多个输入合并：

```rust
pub enum MergeStrategy {
    /// 将所有上游输出合并为一个 JSON 对象（按节点 ID 分字段）
    ByNodeId,
    /// 将所有上游输出合并为一个数组
    Array,
    /// 使用自定义合并函数
    Custom { merge_fn: String },
}

impl DataFlowManager {
    /// 合并多个上游的输出
    pub fn merge_inputs(
        &self,
        upstream_outputs: HashMap<String, serde_json::Value>,
        strategy: MergeStrategy,
    ) -> serde_json::Value {
        match strategy {
            MergeStrategy::ByNodeId => {
                serde_json::json!(upstream_outputs)
            }
            MergeStrategy::Array => {
                serde_json::Value::Array(
                    upstream_outputs.into_values().collect()
                )
            }
            MergeStrategy::Custom { .. } => {
                // 预留：使用 Lua/JS 脚本进行自定义合并
                unimplemented!("Custom merge not yet implemented")
            }
        }
    }
}
```

### 6.3 数据转换管线

```
上游输出 ──▶ 字段提取 ──▶ 格式转换 ──▶ 合并 ──▶ 下游输入

示例：
NodeA output: { "users": [...], "meta": {...} }
    │
    ├── extract: ["users"]
    │
    ▼
NodeB input: { "users": [...] }
```

---

## 7. 与现有系统的集成

### 7.1 新增工具

在现有 Tool 系统下注册以下新工具：

| 工具名 | 用途 | 参数 |
|--------|------|------|
| `pipeline_build` | 构建一个 DAG Pipeline 定义 | `nodes`, `edges`, `config` |
| `pipeline_execute` | 执行一个 Pipeline | `pipeline_id`, `input` |
| `pipeline_status` | 查询 Pipeline 执行状态 | `pipeline_id` |
| `pipeline_list` | 列出所有 Pipeline | 无 |

### 7.2 AgentBuilder 扩展

为 AgentBuilder 添加 DAG 相关配置：

```rust
impl AgentBuilder {
    /// 设置 DAG 工作 Agent 的模型（不设置则使用主 Agent 的模型）
    pub fn worker_model(mut self, model: Box<dyn ModelAdapter>) -> Self;

    /// 设置 DAG 审核 Agent 的模型
    pub fn reviewer_model(mut self, model: Box<dyn ModelAdapter>) -> Self;

    /// 设置默认 Pipeline 配置
    pub fn pipeline_config(mut self, config: PipelineConfig) -> Self;
}
```

### 7.3 TaskManager 集成

通过 TaskManager 追踪 DAG 执行状态：

```
TaskManager 持有 DAG 执行状态：
- 当前运行的 Pipeline ID
- 各节点状态摘要
- 审核失败详情
```

---

## 8. 状态持久化

### 8.1 文件结构

```
dag_runs/
├── <pipeline_id>/
│   ├── pipeline_def.json        ← Pipeline 定义（不可变）
│   ├── execution_state.json     ← 运行时状态（可变，用于恢复）
│   ├── nodes/
│   │   ├── <node_id>/
│   │   │   ├── input.json       ← 节点输入
│   │   │   ├── worker_output.json  ← Worker 输出
│   │   │   ├── review_result.json  ← 审核结果
│   │   │   └── logs/            ← 执行日志
│   │   └── ...
│   └── final_output.json        ← 最终输出
```

### 8.2 断点续跑

支持从失败点恢复执行：

```rust
impl DAGEngine {
    /// 从持久化状态恢复执行
    pub async fn resume(pipeline_id: &str) -> Result<Self> {
        // 1. 加载 pipeline_def.json
        // 2. 加载 execution_state.json
        // 3. 找出所有未完成的节点
        // 4. 重新构建 DAGEngine 并继续执行
    }
}
```

---

## 9. 事件与可观测性

### 9.1 事件系统

```rust
/// DAG 运行时事件
pub enum DAGEvent {
    /// Pipeline 开始执行
    PipelineStarted { id: String, total_nodes: usize },
    /// 节点状态变更
    NodeStatusChanged { node_id: String, old_status: NodeStatus, new_status: NodeStatus },
    /// Worker 开始执行
    WorkerStarted { node_id: String },
    /// Worker 完成
    WorkerCompleted { node_id: String, duration: f64 },
    /// Reviewer 开始审核
    ReviewerStarted { node_id: String },
    /// 审核完成
    ReviewCompleted { node_id: String, passed: bool, score: f32 },
    /// 节点重试
    NodeRetrying { node_id: String, attempt: u32, reason: String },
    /// Pipeline 完成
    PipelineCompleted { id: String, total_duration: f64, node_count: usize },
    /// Pipeline 失败
    PipelineFailed { id: String, error: String, failed_node: String },
}
```

### 9.2 日志与追踪

```rust
/// 节点执行过程中的日志条目
pub struct NodeLog {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: LogLevel,
    pub source: LogSource,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
}

pub enum LogSource {
    Engine,
    Worker,
    Reviewer,
    DataFlow,
}

pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}
```

---

## 10. 使用示例

### 10.1 定义 Pipeline

```rust
// 代码定义方式
let pipeline = PipelineDef::new("data-pipeline", "数据处理流水线")
    .add_node(NodeDef::new("fetch", "数据获取")
        .worker_instruction("从 API 获取用户数据")
        .review_criteria(ReviewCriteria::new()
            .check("数据格式必须为 JSON")
            .check("必须包含必要字段: id, name, email"))
        .input_mode(InputMode::RawInput)
        .output_mode(OutputMode::Json { schema: None }))
    .add_node(NodeDef::new("transform", "数据转换")
        .worker_instruction("清洗和转换用户数据，去除重复项")
        .review_criteria(ReviewCriteria::new()
            .check("无重复记录")
            .check("数据符合目标 schema"))
        .input_mode(InputMode::Merged))
    .add_node(NodeDef::new("analyze", "数据分析")
        .worker_instruction("对清洗后的数据进行分析统计")
        .review_criteria(ReviewCriteria::new()
            .check("分析结果准确")
            .check("图表描述清晰"))
        .input_mode(InputMode::Merged))
    .add_node(NodeDef::new("report", "生成报告")
        .worker_instruction("基于分析结果生成最终报告")
        .review_criteria(ReviewCriteria::new()
            .check("报告格式完整")
            .check("语言专业"))
        .input_mode(InputMode::Merged))
    .add_edge(EdgeDef::new("fetch", "transform"))
    .add_edge(EdgeDef::new("transform", "analyze"))
    .add_edge(EdgeDef::new("transform", "report"))
    .add_edge(EdgeDef::new("analyze", "report"))
    .config(PipelineConfig {
        max_concurrency: 2,
        node_timeout_seconds: 300,
        max_review_retries: 2,
        ..Default::default()
    });
```

### 10.2 通过工具定义 Pipeline

通过 `pipeline_build` 工具，用户可以直接通过自然语言描述定义 DAG：

```
用户: 构建一个数据处理流水线，先获取数据，然后清洗，
      清洗完成后并行进行分析和报表生成

Agent → 调用 pipeline_build(nodes=[...], edges=[...])
      → 返回 pipeline_id
      → 调用 pipeline_execute(pipeline_id="pl_xxx", input="https://api.example.com/data")
```

### 10.3 执行流程可视化

```
Pipeline: 数据处理流水线
─────────────────────────────────────────────────
 fetch ──────▶ transform ──────┬────▶ analyze ───┐
                                │                  │
                                └────▶ report ─────┴──▶ ✅ Done
─────────────────────────────────────────────────
  状态: [✅ 完成] [✅ 完成] [✅ 完成] [✅ 完成]

节点详情:
  fetch:      ✅ 完成 (2.3s)  审核: 通过 (评分 4.8/5)
  transform:  ✅ 完成 (5.1s)  审核: 通过 (评分 4.5/5)
  analyze:    ✅ 完成 (8.7s)  审核: 通过 (评分 4.2/5)
  report:     ✅ 完成 (6.4s)  审核: 通过 (评分 4.6/5)

总耗时: 22.5s
```

---

## 11. 实现路线图

### Phase 1 — 基础框架（预计 1-2 天）

- [ ] 创建 `src/dag/` 目录结构
- [ ] 实现核心数据模型（`PipelineDef`, `NodeDef`, `EdgeDef`, 状态枚举）
- [ ] 实现拓扑排序与环检测
- [ ] 单元测试覆盖基础逻辑

### Phase 2 — 引擎与执行（预计 2-3 天）

- [ ] 实现 `DAGEngine` — 调度器主循环
- [ ] 实现 `NodeSupervisor` — 节点内部协调
- [ ] 实现 `WorkerAgent` 封装
- [ ] 实现 `ReviewerAgent` 封装
- [ ] 实现 `DataFlowManager` — 数据传递与合并
- [ ] 集成测试：单链 DAG、分支 DAG

### Phase 3 — 工具与集成（预计 1-2 天）

- [ ] 创建 `dag_tools` 工具集
- [ ] 实现 `pipeline_build` 工具
- [ ] 实现 `pipeline_execute` 工具
- [ ] 实现 `pipeline_status` / `pipeline_list` 工具
- [ ] AgentBuilder 扩展
- [ ] TaskManager 集成

### Phase 4 — 增强与打磨（预计 1-2 天）

- [ ] 断点续跑
- [ ] 事件系统
- [ ] 审核重试策略优化
- [ ] 可视化日志输出
- [ ] Pipeline 模板与复用

---

## 12. 关键设计决策

### 12.1 为什么每个节点使用独立 Agent 实例？

| 方案 | 优点 | 缺点 |
|------|------|------|
| ✅ **独立 Agent 实例** | 隔离性好，不会相互污染上下文；可独立配置模型 | 资源开销较大 |
| ❌ **共享 Agent + 上下文切换** | 节省资源 | 上下文管理复杂，容易相互干扰 |

**决策**: 采用独立 Agent 实例方案。Worker 和 Reviewer 各自拥有独立的 `Agent` 实例、独立的上下文管理器和工具集。这保证了工作质量和审核的独立性。

### 12.2 Worker 和 Reviewer 是否使用相同模型？

| 方案 | 优点 | 缺点 |
|------|------|------|
| ✅ **不同模型** | Worker 用强模型（如 DeepSeek V4）保证质量，Reviewer 用轻模型（如 DeepSeek V3）降低成本 | 配置复杂 |
| ❌ **相同模型** | 配置简单 | 可能浪费资源 |

**决策**: 默认使用相同模型，但支持独立配置。`PipelineConfig` 提供 `worker_model` 和 `reviewer_model` 可选字段。

### 12.3 审核不通过的处理策略

| 方案 | 优点 | 缺点 |
|------|------|------|
| ✅ **带反馈重试** | Worker 能根据审核反馈改进 | 可能陷入无限循环 |
| ❌ **直接跳过** | 快速失败 | 可能错过可修复的问题 |
| ❌ **人工介入** | 最准确 | 破坏自动化 |

**决策**: 采用**带反馈重试**策略，设置最大重试次数（默认 3 次）。每次重试时将审核反馈注入 Worker 的上下文，让 Worker 根据反馈修正。超过最大重试次数后标记为 Failed 或 Skipped。

### 12.4 数据传递方式

**决策**: 使用**值传递**（而非引用传递），每个节点的输出完整复制到下游节点的输入中。这种方式虽然占用更多内存，但保证了数据隔离，便于断点续跑和调试。

---

## 13. 风险与应对

| 风险 | 影响 | 应对措施 |
|------|------|---------|
| DAG 中存在环 | 死循环 | 拓扑排序时检测环，抛出明确的错误 |
| Worker 执行超时 | 阻塞整个 Pipeline | 设置 `node_timeout_seconds`，超时后标记为 Failed |
| 审核过于严格 | 频繁重试，效率低 | 可配置审核标准，支持宽松模式 |
| 资源消耗过大 | 多 Agent 实例同时运行可能耗尽资源 | `max_concurrency` 控制并行度 |
| 上下文爆炸 | 长 Pipeline 数据传递导致上下文过大 | 下游只接收上游输出的摘要或关键字段，而非完整数据 |

---

> **下一步**: 实现 Phase 1 基础框架，包括核心数据模型和拓扑排序。
