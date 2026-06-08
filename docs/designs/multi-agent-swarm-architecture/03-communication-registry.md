# 🐝 多 Agent 蜂群架构设计（Multi-Agent Swarm Architecture） — 通信、注册与发现

> 原文拆分自 `../multi-agent-swarm-architecture.md`。


### 5.1 通信协议 — JSON-RPC over Unix Domain Socket

```json
// Request
{
  "jsonrpc": "2.0",
  "id": "req_001",
  "method": "dispatch_task",
  "params": {
    "task_type": "code_search",
    "payload": {
      "pattern": "fn execute",
      "path": "./src"
    },
    "timeout_ms": 30000
  }
}

// Response (success)
{
  "jsonrpc": "2.0",
  "id": "req_001",
  "result": {
    "status": "completed",
    "data": { "matches": [...], "count": 42 }
  }
}

// Response (error)
{
  "jsonrpc": "2.0",
  "id": "req_001",
  "error": {
    "code": -32000,
    "message": "Task timed out",
    "data": { "elapsed_ms": 30000 }
  }
}
```

### 5.2 事件推送协议

Memory Agent 向 Orchestrator 推送事件：

```json
{
  "jsonrpc": "2.0",
  "method": "event",
  "params": {
    "event": "memory_saved",
    "from": "memory",
    "data": {
      "memory_id": "mem_abc123",
      "content_preview": "项目使用了 Actix-web 框架",
      "importance": 0.85
    }
  }
}
```

### 5.3 Agent 心跳

每 10 秒 Agent 向 Orchestrator 发送心跳：

```json
{
  "jsonrpc": "2.0",
  "method": "heartbeat",
  "params": {
    "agent_id": "memory_001",
    "agent_type": "memory",
    "status": "idle",
    "uptime_secs": 3600,
    "tasks_completed": 42,
    "memory_usage_mb": 128
  }
}
```

### 5.4 通信架构图

```
┌─────────────────────────────────────────────────────────┐
│                   Unix Domain Socket                      │
│              /tmp/agent-lab/swarm.sock                    │
│                                                          │
│  ┌──────────────┐     ┌──────────────┐                  │
│  │ Orchestrator │◄───►│ Memory Agent │                  │
│  │   (server)   │     │   (client)   │                  │
│  └──────┬───────┘     └──────────────┘                  │
│         │                                                │
│    ┌────┼──────────────────────────────────┐             │
│    │    │      Agent Connection Pool        │             │
│    │    ├──────────┐  ┌──────────┐          │             │
│    │    │ General  │  │ General  │  ...      │             │
│    │    │ Agent #1 │  │ Agent #2 │          │             │
│    │    └──────────┘  └──────────┘          │             │
│    │    ┌──────────┐  ┌──────────┐          │             │
│    │    │Verifier  │  │ Reader   │  ...      │             │
│    │    │ Agent    │  │ Agent    │          │             │
│    │    └──────────┘  └──────────┘          │             │
│    └────────────────────────────────────────┘             │
└─────────────────────────────────────────────────────────┘
```

---

## 6. Agent 注册与发现

### 6.1 Swarm Registry（蜂群注册表）

Orchestrator 维护一个全局注册表，记录所有活跃 Agent 的信息：

```rust
/// 蜂群注册表 — 管理所有 Agent 的生命周期与发现
pub struct SwarmRegistry {
    /// agent_id → AgentInfo
    agents: HashMap<String, AgentInfo>,
    /// agent_type → [agent_id, ...]（快速按类型查询）
    type_index: HashMap<AgentType, Vec<String>>,
    /// Agent 心跳监控
    heartbeat_monitor: HeartbeatMonitor,
}

/// Agent 信息
pub struct AgentInfo {
    pub agent_id: String,           // 唯一标识 "memory_001"
    pub agent_type: AgentType,      // Agent 类型
    pub status: AgentStatus,        // 当前状态
    pub socket_path: PathBuf,       // Agent 监听的 socket 路径
    pub pid: u32,                   // 进程 ID
    pub started_at: chrono::DateTime<Utc>,
    pub capabilities: Vec<Capability>,  // 能力列表
    pub metadata: HashMap<String, String>, // 扩展元数据
}

/// Agent 类型枚举
pub enum AgentType {
    Orchestrator,
    Memory,
    General,
    Verifier,
    Reader,
    Coder,
    Custom(String),
}

/// Agent 运行状态
pub enum AgentStatus {
    Starting,
    Idle,           // 空闲，等待任务
    Busy,           // 正在执行任务
    Degraded,       // 降级运行
    Stopped,        // 已停止
    Failed,         // 异常退出
}
```

### 6.2 注册流程

```
Orchestrator                    New Agent
     │                              │
     │  1. spawn agent process      │
     │─────────────────────────────►│
     │                              │
     │  2. Agent 启动，监听 socket   │
     │                              │
     │  3. register(capabilities)   │
     │◄─────────────────────────────│
     │                              │
     │  4. 验证身份，分配 agent_id   │
     │─────────────────────────────►│
     │                              │
     │  5. heartbeat(10s间隔)       │
     │◄─────────────────────────────│
     │                              │
     │  6. 加入 Agent Pool          │
     │  更新 SwarmRegistry          │
     │                              │
```

### 6.3 Agent 发现

Orchestrator 可以通过以下方式发现可用 Agent：

```rust
// 按类型查找所有空闲 Agent
let idle_general_agents = registry.find_agents(AgentType::General, AgentStatus::Idle);

// 按能力查找
let agents_with_capability = registry.find_by_capability(Capability::CodeSearch);

// 获取所有 Agent 状态（用于 /swarm status 命令）
let swarm_status = registry.get_all_status();
```

---

## 7. 任务编排与派发
