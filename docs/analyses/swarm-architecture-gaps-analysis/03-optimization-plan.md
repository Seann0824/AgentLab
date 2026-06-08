# 🐝 多 Agent 蜂群架构—分阶段优化计划与优先级

> 原文拆分自 `../swarm-architecture-gaps-analysis.md`。

## 1. 阶段总览

```
阶段 1 (P0) ─────────────────────────────────────────────────
  [SwarmTask 模型] → [dispatch_task 工具] → [任务回传机制]
  
  目标: 实现 Orchestrator → Agent 的首次「真实任务派发」
  交付: 跑通完整的「派发→执行→回传」链路

阶段 2 (P1) ─────────────────────────────────────────────────
  [Workflow 真实执行] + [Pool + Orchestrator 整合]
  
  目标: Workflow 能驱动真实 Agent 执行任务
  交付: 一个端到端的并行调研 Workflow

阶段 3 (P2) ─────────────────────────────────────────────────
  [容错与恢复] + [监控与可视化]
  
  目标: 生产级稳定性
  交付: 断线重连、自动重启、Web 监控面板
```

---

## 2. 阶段 1（P0）：核心任务派发链路

### 2.1 步骤 1：引入 SwarmTask 模型

**文件：** `src/swarm/task.rs`（新文件）

```rust
/// 蜂群任务 — 可派发给任意 Agent 执行的工作单元
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub task_id: String,
    pub task_type: String,
    pub target_agent_type: AgentType,
    pub payload: serde_json::Value,
    pub priority: TaskPriority,
    pub timeout_seconds: u64,
    pub max_retries: u32,
    pub status: TaskStatus,
    pub created_at: u64,
    pub agent_id: Option<String>,
    pub result: Option<TaskResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub started_at: u64,
    pub completed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending, Running, Completed, Failed, Cancelled, TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskPriority {
    Low, Normal, High, Critical,
}
```

**模块集成：**

```rust
// src/swarm/mod.rs 新增
pub mod task;
pub use task::{SwarmTask, TaskResult, TaskStatus, TaskPriority};
```

### 2.2 步骤 2：实现 dispatch_task 工具

**文件：** `src/tools/dispatch_task.rs`（新文件）

```
工具名: dispatch_task
描述: 向指定 Agent 或 Agent 类型派发任务，等待执行结果
参数:
  - agent_type: string (必填) — 目标 Agent 类型 (memory/general/verifier/coder/researcher)
  - task_description: string (必填) — 任务描述
  - timeout_seconds: integer (可选, 默认 60) — 超时时间
  - wait_result: boolean (可选, 默认 true) — 是否等待结果
```

**关键实现逻辑：**

```rust
impl Tool for DispatchTask {
    fn execute(&self, args: serde_json::Value) -> ToolStream {
        // 1. 解析参数 → SwarmTask
        // 2. 通过 SwarmRegistry 查找目标 Agent（按类型找空闲 Agent）
        // 3. 通过 Orchestrator 的 send_request() 发送 dispatch_task RPC
        // 4. 等待响应（通过 oneshot channel）
        // 5. 返回结果
    }
}
```

**注册到 ToolManager：**

```rust
// src/agent/default_tools.rs 或类似位置
tool_manager.register_tool(Box::new(DispatchTask {
    swarm_registry: registry.clone(),
    orchestrator: orch_arc.clone(),  // Arc<Mutex<SwarmOrchestrator>>
}));
```

### 2.3 步骤 3：任务回传机制

**修改 `SwarmOrchestrator`：**

```rust
// 新增: 等待 Agent 响应的 pending 请求表
pub struct SwarmOrchestrator {
    // ... 现有字段 ...
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
}

// 新增: 后台读取任务（在 accept_loop 中处理）
async fn reader_loop(orchestrator: Arc<Mutex<Self>>) {
    loop {
        // 对于每个已连接的 Agent 流，并行读取响应
        // 收到响应后，通过 pending_requests 中的 oneshot 通知调用方
    }
}
```

**修改 `dispatch_task` 返回值：**

```rust
// 不再返回 tool_progress("任务已发送")
// 而是阻塞等待 Agent 返回结果：
// 1. 创建 oneshot channel
// 2. 存入 pending_requests
// 3. send_request
// 4. await oneshot receiver
// 5. 返回结果给 LLM
```

### 2.4 阶段 1 验证标准

```bash
# 1. 启动 Orchestrator
cargo run -- --agent-type orchestrator

# 2. 验证 Memory Agent 自动启动并注册
# 预期: "🧠 Memory Agent 连接到 Orchestrator"

# 3. 在 Orchestrator 中（或通过 spawn_agent 子进程）调用 dispatch_task
# 预期: 任务被派发到指定 Agent → 执行 → 返回结果
```

---

## 3. 阶段 2（P1）：Workflow 真实执行 + Pool 整合

### 3.1 步骤 4：Workflow 真实执行

**修改 `src/swarm/workflow/execution.rs`：**

```rust
// 替换 Mock 实现为真实 UDS 派发
pub(super) async fn execute_step(
    pool_manager: Arc<TokioMutex<AgentPoolManager>>,
    task: &str,
    step_name: String,
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,  // 新增参数
) -> Result<String> {
    // 1. 从 Pool 获取 Agent 实例
    let instance = pool_manager.lock().await.general_pool.acquire().await?;
    
    // 2. 创建 SwarmTask
    let swarm_task = SwarmTask::new("workflow", task, AgentType::General);
    
    // 3. 通过 Orchestrator 派发任务
    let orch = orchestrator.lock().await;
    let response = orch.send_request(&instance.id, &swarm_task).await?;
    
    // 4. 释放实例
    pool_manager.lock().await.general_pool.release(&instance.id).await?;
    
    // 5. 返回结果
    Ok(response.to_string())
}
```

**修改 `WorkflowEngine` 构造函数：**

```rust
// 新增参数
pub struct WorkflowEngine {
    pool_manager: Arc<TokioMutex<AgentPoolManager>>,
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,  // 新增
    active_workflows: Arc<TokioMutex<HashMap<String, WorkflowState>>>,
}
```

### 3.2 步骤 5：Pool + Orchestrator 整合

**统一注册路径：**

```rust
// 方案：AgentInstance 创建时，同时在两个地方注册
// 1. 在 Pool 中作为实例
// 2. 在 Orchestrator 的 registry 和 streams 中

impl AgentPool {
    async fn spawn_instance(&self, index: usize) -> Result<AgentInstance> {
        let client = UdsClient::connect(&self.orchestrator_socket, &agent_id).await?;
        
        // ✅ 新增：在 Orchestrator 中注册（通过 UDS 发送 register 消息）
        client.send_request(&JsonRpcRequest::new("register", ...)).await?;
        
        Ok(AgentInstance {
            id: agent_id,
            status: AgentInstanceStatus::Idle,
            client: Some(Arc::new(TokioMutex::new(client))),
            // ...
        })
    }
}
```

**任务路由策略：**

```rust
// 方案：dispatch_task 工具的内部路由
fn select_target_agent(&self, agent_type: &AgentType) -> Option<String> {
    // 1. 优先从 Pool 获取空闲实例
    if let Some(instance) = self.pool_manager.lock().await.acquire_by_type(agent_type) {
        return Some(instance.id);
    }
    
    // 2. 回退：从 Registry 中查找在线 Agent
    let registry = self.orch.lock().await.get_registry_snapshot().await;
    registry.find_idle(agent_type)
}
```

### 3.3 阶段 2 验证标准

```bash
# 1. 定义一个并行调研 Workflow
cargo run -- --agent-type orchestrator
# 在对话中执行:
#   dispatch_task agent_type=general task="搜索 src/tools 目录的 read 相关函数"
#   dispatch_task agent_type=researcher task="分析 src/agent 模块架构"

# 2. 验证 Workflow 能驱动真实 Agent
# 预期: Workflow 中的每个 step 都通过 UDS 派发到实际 Agent 执行
```

---

## 4. 阶段 3（P2）：容错与可观测性

### 4.1 步骤 6：断线重连

```rust
// Agent 端：检测到断开后自动重连
impl MemoryAgent {
    async fn run(&mut self, ...) {
        loop {
            match self.client.as_ref() {
                Some(client) => {
                    match client.read_request().await {
                        Ok(req) => self.handle_request(req).await,
                        Err(_) => {
                            // 连接断开，尝试重连
                            eprintln!("🧠 连接断开，5秒后重连...");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            self.connect(orchestrator_socket.clone()).await?;
                        }
                    }
                }
                None => {
                    self.connect(orchestrator_socket.clone()).await?;
                }
            }
        }
    }
}
```

### 4.2 步骤 7：Agent 自动重启

```rust
// Orchestrator 端：检测到心跳超时后自动重启
impl SwarmOrchestrator {
    async fn restart_agent(&mut self, agent_id: &str) -> Result<()> {
        // 1. 标记为 Offline
        self.registry.lock().await.mark_offline(agent_id);
        
        // 2. 获取 Agent 类型
        let agent_type = self.agent_types.get(agent_id).cloned();
        
        // 3. 启动新子进程替代
        if let Some(agent_type) = agent_type {
            self.spawn_subprocess(agent_type, ...).await?;
        }
        
        Ok(())
    }
}
```

### 4.3 步骤 8：可观测性增强

```text
1. 任务历史追踪 — SwarmTask 持久化到文件/DB
2. 实时 Agent 状态 — 增强 SwarmCtl 输出（CPU/内存/任务数）
3. 事件日志 — 所有 RPC 调用记录到环形缓冲区
4. 健康检查 API — 通过 UDS 查询整体蜂群健康状态
```

### 4.4 阶段 3 验证标准

```bash
# 1. 手动 kill Memory Agent → 验证自动重启
# 2. kill Orchestrator → 验证子 Agent 重连
# 3. 连续 dispatch 100 个任务 → 验证无资源泄漏
```

---

## 5. 关键设计决策

### 5.1 dispatch_task 是否同步等待结果？

| 方案 | 优点 | 缺点 |
|------|------|------|
| **同步等待**（推荐） | 调用简单，适合 LLM 使用 | 阻塞直到超时 |
| 异步派发 | 不阻塞，支持批量 | 调用方需要额外查结果 |

**决策：** 默认同步等待，支持 `wait_result=false` 参数做异步派发。

### 5.2 如何选取目标 Agent？

| 方案 | 优点 | 缺点 |
|------|------|------|
| **按类型+负载均衡**（推荐） | 简单公平 | 无智能路由 |
| 按能力匹配 | 精准 | 需要能力注册机制 |
| 轮询 | 均匀 | 忽略负载差异 |

**决策：** 第一阶段按类型选取空闲 Agent，后续引入负载均衡。

### 5.3 WorkflowEngine 如何获取 Orchestrator 引用？

| 方案 | 优点 | 缺点 |
|------|------|------|
| **构造函数注入**（推荐） | 明确依赖 | 需要修改已有代码 |
| 全局单例 | 无需修改签名 | 测试困难 |

**决策：** 构造函数注入 `Arc<TokioMutex<SwarmOrchestrator>>`。

### 5.4 Pool 与 Registry 的关系？

| 方案 | 优点 | 缺点 |
|------|------|------|
| **Pool 作为 Registry 的子集**（推荐） | 统一管理 | 需同步 |
| 各自独立 | 解耦 | 维护两套状态 |

**决策：** Pool 管理的实例自动注册到 Registry，Registry 作为全局视图。

---

## 6. 预估进度与里程碑

| 里程碑 | 内容 | 预估工时 | 交付物 |
|--------|------|---------|--------|
| **M1** | SwarmTask 模型 | ~1h | `src/swarm/task.rs` |
| **M2** | dispatch_task 工具 | ~3h | `src/tools/dispatch_task.rs` + 注册 |
| **M3** | 任务回传机制 | ~2h | pending_requests + reader_loop |
| **M4** | Workflow 真实执行 | ~2h | 替换 execution.rs Mock |
| **M5** | Pool + Orchestrator 整合 | ~3h | 统一注册 + 路由策略 |
| **M6** | 容错与可观测性 | ~4h | 重连/重启/监控 |
