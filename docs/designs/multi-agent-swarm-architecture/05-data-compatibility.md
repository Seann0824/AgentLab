# 🐝 多 Agent 蜂群架构设计（Multi-Agent Swarm Architecture） — 数据结构、兼容性与风险

> 原文拆分自 `../multi-agent-swarm-architecture.md`。

## 10. 数据结构设计

### 10.1 目录结构

```
src/
├── main.rs                    # 入口：支持 --agent-type 参数
├── agent.rs                   # ⭐ Orchestrator Agent（增强现有 Agent）
├── swarm/                     # 🆕 蜂群模块（新目录）
│   ├── mod.rs                 # 模块入口
│   ├── transport.rs           # UDS 传输层
│   ├── rpc.rs                 # JSON-RPC 协议
│   ├── registry.rs            # SwarmRegistry
│   ├── heartbeat.rs           # 心跳检测
│   ├── pool.rs                # Agent 连接池
│   ├── workflow.rs            # 任务编排
│   └── agents/                # 🆕 Agent 角色实现
│       ├── mod.rs
│       ├── memory.rs          # Memory Agent 逻辑
│       ├── general.rs         # General Agent 逻辑
│       └── verifier.rs        # Verifier Agent 逻辑
├── tools/
│   ├── mod.rs
│   ├── dispatch_task.rs       # 🆕 派发任务工具
│   └── swarm_ctl.rs           # 🆕 蜂群控制工具
├── context/
├── memory/
├── model/
├── goal/
└── task/
```

### 10.2 核心结构体

```rust
// ============================================================
// src/swarm/transport.rs — UDS 传输层
// ============================================================

/// UDS 服务器（Orchestrator 使用）
pub struct UdsServer {
    listener: tokio::net::UnixListener,
    connections: HashMap<String, UdsConnection>,
}

/// UDS 客户端（Agent 使用）
pub struct UdsClient {
    stream: tokio::net::UnixStream,
    agent_id: String,
}

/// 连接信息
pub struct UdsConnection {
    pub agent_id: String,
    pub stream: tokio::net::UnixStream,
    pub connected_at: chrono::DateTime<Utc>,
}

// ============================================================
// src/swarm/rpc.rs — JSON-RPC 2.0 协议
// ============================================================

/// JSON-RPC 请求
#[derive(Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,           // "2.0"
    pub id: String,                // 请求 ID
    pub method: String,            // 方法名
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 响应
#[derive(Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,           // "2.0"
    pub id: String,                // 对应请求 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 错误
#[derive(Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// 预定义的 RPC 方法
pub enum SwarmMethod {
    // Agent 注册与发现
    Register,           // Agent → Orchestrator
    Unregister,         // Agent → Orchestrator
    Heartbeat,          // Agent → Orchestrator
    QuerySwarm,         // Orchestrator → 自身

    // 任务派发
    DispatchTask,       // Orchestrator → Agent
    CancelTask,         // Orchestrator → Agent
    TaskResult,         // Agent → Orchestrator

    // 事件
    Event,              // Agent → Orchestrator
    Broadcast,          // Orchestrator → Agent
}

// ============================================================
// src/swarm/workflow.rs — 任务编排
// ============================================================

/// 工作流定义
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

/// 工作流步骤
pub struct WorkflowStep {
    pub id: String,
    pub agent_type: AgentType,
    pub task_description: String,
    pub timeout: Duration,
    pub on_success: Option<String>,  // 下一步 ID 或 "complete"
    pub on_failure: Option<String>,  // 下一步 ID 或 "abort"
    pub retry_count: u32,
}

/// 工作流执行器
pub struct WorkflowExecutor {
    workflow: Workflow,
    state: WorkflowState,
    results: HashMap<String, TaskResult>,
}

pub enum WorkflowState {
    Pending,
    Running { current_step_id: String },
    Completed,
    Failed { failed_step_id: String, error: String },
    Cancelled,
}
```

---

## 11. 向后兼容性分析

### 11.1 兼容策略

| 变更 | 影响 | 兼容措施 |
|------|------|---------|
| 新增 `swarm/` 模块 | 无影响 | 新模块，不修改现有模块 |
| 增强 `agent.rs` | 主 Agent 增加蜂群能力 | 通过 Feature Flag 控制 |
| 新增工具 `dispatch_task` | 无影响 | 新工具注册，不影响现有工具 |
| 新增 `--agent-type` 参数 | 无影响 | 默认 `orchestrator`，现有用法不变 |
| 创建新的 Agent 类型 | 无影响 | 各 Agent 独立运行，互不干扰 |

### 11.2 Feature Flag

```toml
[features]
default = ["swarm"]
swarm = []          # 蜂群架构（默认启用）
legacy = []         # 纯单 Agent 模式（不使用蜂群）
```

### 11.3 启动参数兼容

```bash
# 现有用法完全不变
agent-lab                           # 启动 Orchestrator（默认）

# 新用法
agent-lab --agent-type memory       # 启动 Memory Agent
agent-lab --agent-type general      # 启动 General Agent
agent-lab --agent-type verifier     # 启动 Verifier Agent

# 指定 socket 路径
agent-lab --agent-type general --socket /tmp/agent-lab/g1.sock
```

---

## 12. 风险与应对

### 12.1 风险矩阵

| 风险 | 概率 | 影响 | 应对措施 |
|------|------|------|---------|
| UDS 通信不稳定 | 低 | 高 | 自动重连、心跳超时重试、熔断机制 |
| Agent 进程崩溃 | 中 | 中 | 自动重启、任务重试、错误日志 |
| 资源消耗过高 | 中 | 中 | Agent Pool 大小限制、空闲回收 |
| 消息顺序错乱 | 低 | 高 | 任务 ID 追踪、幂等设计 |
| 向量数据库竞争 | 低 | 中 | Memory Agent 单例、写锁 |
| 编译验证循环死锁 | 中 | 低 | 最大迭代次数限制、超时熔断 |

### 12.2 关键设计决策（记录到 MEMORY.md）

| 决策 | 选项 | 选择 | 原因 |
|------|------|------|------|
| IPC 协议 | gRPC / MQ / UDS | **UDS + JSON-RPC** | 轻量、零依赖、进程级通信足够 |
| Agent 进程管理 | Fork / Spawn | **Spawn** | Rust 的 `Command::new` 更安全 |
| 任务序列化 | JSON / MessagePack / Protobuf | **JSON** | 调试友好、与现有 serde_json 一致 |
| Agent 池管理 | 固定池 / 动态池 | **动态池** | 灵活、适应多种负载 |
| 工作流定义 | DSL / 代码 / JSON | **JSON** | 可序列化、可持久化、便于调试 |

---

