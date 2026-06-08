# 🐝 多 Agent 蜂群架构设计（Multi-Agent Swarm Architecture）

> **版本**: v2.0 — 蜂群升级
> **创建日期**: 2025-06-08
> **状态**: 📋 设计阶段
> **对应路线图**: Phase 4 — 多 Agent 蜂群
> **依赖前置**: Phase 1~3（Agent 内核、上下文管理、持久化记忆）

---

## 目录

1. [设计目标与动机](#1-设计目标与动机)
2. [当前架构分析与局限](#2-当前架构分析与局限)
3. [整体架构设计](#3-整体架构设计)
4. [Agent 角色定义](#4-agent-角色定义)
5. [Agent 间通信模式](#5-agent-间通信模式)
6. [Agent 注册与发现](#6-agent-注册与发现)
7. [任务编排与派发](#7-任务编排与派发)
8. [Agent 生命周期管理](#8-agent-生命周期管理)
9. [实现阶段划分](#9-实现阶段划分)
10. [数据结构设计](#10-数据结构设计)
11. [向后兼容性分析](#11-向后兼容性分析)
12. [风险与应对](#12-风险与应对)

---

## 1. 设计目标与动机

### 1.1 核心动机

当前系统已经具备了良好的单 Agent 能力：

- ✅ 上下文压缩与摘要管理
- ✅ 持久化记忆（向量搜索）
- ✅ 目标驱动（Goal-Driven）模式
- ✅ 工具系统（Tool System）
- ✅ 会话管理（Session）
- ✅ 子进程 Agent（spawn_agent）

但当前的「主从 Agent 架构」有本质局限：

| 现状 | 问题 |
|------|------|
| Sub-agent 每次 `cargo build` 全新编译 | 启动慢、资源浪费 |
| Sub-agent 无状态、用完即弃 | 无法积累经验与上下文 |
| 所有工具在同一进程中执行 | 没有隔离、没有优先级 |
| 所有任务由主 Agent 手工派发 | 不能异步、不能并行 |
| 没有 Agent 注册与发现机制 | 无法扩展 Agent 类型 |

### 1.2 设计目标

1. **Agent 角色化** — 定义不同职责的 Agent 类型（Memory Agent、General Agent、验证 Agent 等）
2. **进程级隔离** — 每个 Agent 运行在独立进程中，通过 IPC 通信
3. **异步自动派发** — Memory Agent 后台自动运行，不阻塞主流程
4. **Agent 注册与发现** — Agent 启动时自动注册，主 Agent 可查询可用 Agent 列表
5. **任务编排** — 支持串行、并行、条件分支的任务编排
6. **状态持久化** — Agent 状态可保存、恢复、迁移

### 1.3 非目标

- 不实现分布式跨机器通信（仅限本地进程间）
- 不替换现有单 Agent 模式（兼容旧模式）
- 不引入第三方消息队列（使用 Unix Domain Socket / 文件 IPC）

---

## 2. 当前架构分析与局限

### 2.1 当前架构

```
┌─────────────────────────────────────────────┐
│               主 Agent (当前进程)              │
│  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
│  │ Context  │  │  Tools   │  │  Memory    │  │
│  │ Manager  │  │ Manager  │  │  Manager   │  │
│  └──────────┘  └──────────┘  └────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
│  │  Goal    │  │  Task    │  │  Session   │  │
│  │ Registry │  │ Manager  │  │  Manager   │  │
│  └──────────┘  └──────────┘  └────────────┘  │
└────────────────────┬────────────────────────┘
                     │ spawn_agent
                     ▼
        ┌────────────────────────┐
        │  Sub-agent (子进程)     │
        │  全新编译 + 全新上下文   │
        │  执行完自动退出          │
        └────────────────────────┘
```

### 2.2 关键局限

1. **无角色分工** — 所有 Agent 用同一份代码、同一套工具，无法针对特定任务优化
2. **无持久子进程** — 每次 spawn_agent 都要 `cargo build`（~30s+），无法保持长连接
3. **无异步后台** — Memory 保存是同步的，不能异步自动提取
4. **无通信协议** — 只有 stdin/stdout 文本传递，没有结构化 IPC
5. **无编排能力** — 主 Agent 必须手工逐个派发任务，不能定义 workflow

---

## 3. 整体架构设计

### 3.1 蜂群架构概览

```
┌──────────────────────────────────────────────────────────────┐
│                    🐝 Agent Swarm (蜂群)                      │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │           🧠 Orchestrator Agent (调度者)              │   │
│  │  职责: 用户交互入口、任务编排、Agent 调度、结果聚合     │   │
│  │  持有: ContextManager + ToolManager + 蜂群注册表      │   │
│  └────────────┬──────────────┬───────────────────┬───────┘   │
│               │              │                   │           │
│        ┌──────▼───┐   ┌─────▼──────┐   ┌───────▼───────┐  │
│        │ 🧠 Memory │   │ 🔧 General │   │ ✅ Verifier  │  │
│        │   Agent   │   │   Agent    │   │    Agent     │  │
│        ├───────────┤   ├────────────┤   ├──────────────┤  │
│        │ 自动记忆   │   │ 通用任务    │   │ 自我迭代验证  │  │
│        │ 后台异步   │   │ 并行派发    │   │ 编译+测试    │  │
│        │ 长期运行   │   │ 可复用     │   │ 回归验证     │  │
│        └───────────┘   └────────────┘   └──────────────┘  │
│                                                              │
│        ┌───────────┐   ┌────────────┐                       │
│        │ 📖 Reader  │   │ 💻  Code   │   ...more types...   │
│        │   Agent    │   │   Agent    │                       │
│        ├───────────┤   ├────────────┤                       │
│        │ 专注读取    │   │ 专注编码   │                       │
│        └───────────┘   └────────────┘                       │
└──────────────────────────────────────────────────────────────┘
```

### 3.2 通信方式

```
┌─────────────────────────────────────────────┐
│          Agent 间通信协议 (3 层)              │
├─────────────────────────────────────────────┤
│  Layer 3: 任务层 — 任务描述 + 结果 + 状态     │
│  Layer 2: 消息层 — JSON-RPC over IPC        │
│  Layer 1: 传输层 — Unix Domain Socket       │
└─────────────────────────────────────────────┘
```

### 3.3 三种通信模式

| 模式 | 方式 | 适用场景 |
|------|------|---------|
| **同步 RPC** | Agent 发送请求，等待响应 | 一次性任务派发 |
| **异步推送** | Agent 推送事件，不等待 | Memory Agent 后台存储 |
| **广播通知** | Orchestrator 广播给所有 Agent | 配置变更、全局事件 |

### 3.4 进程模型

```
┌─────────────┐
│  Orchestrator │ ← 主进程（现有 agent-lab 进程）
│  (PID=X)      │   监听 Unix Domain Socket
└──────┬───────┘
       │ fork? no — 进程间用 ID 识别
       │
       ├──── Memory Agent ──── PID=X+1 (常驻)
       ├──── General Agent ─── PID=X+2 (按需启动/池化)
       ├──── Verifier Agent ── PID=X+3 (按需)
       └──── Reader Agent ──── PID=X+4 (按需)
```

---

## 4. Agent 角色定义

### 4.1 🧠 Orchestrator Agent（调度者/主 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `orchestrator` |
| **数量** | 1（单例） |
| **生命周期** | 与主进程共存亡 |
| **运行模式** | 交互式（CLI）+ 后台调度 |

**职责：**
1. 用户交互入口（接受输入、显示输出）
2. 维护蜂群注册表（Swarm Registry）
3. 任务编排与派发（Task Dispatching）
4. 结果聚合与冲突解决
5. Agent 健康监控与重启
6. 上下文压缩与任务恢复

**特有工具：**
- `dispatch_task(agent_type, task, params)` — 派发任务给指定 Agent
- `spawn_agent_type(agent_type, config)` — 启动指定类型的新 Agent
- `query_swarm()` — 查询所有活跃 Agent 状态
- `broadcast(event, payload)` — 广播事件

**与当前系统的关系：**
- Orchestrator = 当前的 Agent（主循环），保持完全向后兼容
- 新增蜂群管理能力，但不影响现有的单 Agent 模式

---

### 4.2 📌 Memory Agent（记忆 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `memory` |
| **数量** | 1（单例） |
| **生命周期** | 常驻（随 Orchestrator 启动） |
| **运行模式** | 异步后台 |

**职责：**
1. **自动记忆提取** — 每 N 轮对话自动扫描上下文，提取重要信息存入向量数据库
2. **记忆总结** — 将分散的记忆合并、去重、生成摘要
3. **记忆检索** — 提供高效的内存向量检索服务
4. **记忆遗忘** — 根据重要性评分自动清理低价值记忆
5. **关系挖掘** — 发现记忆之间的关联（如「A 项目用到了 B 技术」）

**特有工具：**
- `auto_extract(context_messages)` — 从对话上下文自动提取可记忆的信息
- `consolidate_memories()` — 合并重复/相似记忆
- `forget_low_importance(threshold)` — 清理低重要性记忆

**与当前 MemoryManager 的关系：**
- 当前 MemoryManager 作为存储层（读/写向量 DB）
- Memory Agent 是上层逻辑：**何时**提取、**什么**值得记、**如何**总结
- Memory Agent 持有一个简化版的 ContextManager（可感知对话上下文）

**启动命令示例：**
```bash
agent-lab --agent-type memory --socket /tmp/agent-lab/memory.sock
```

---

### 4.3 🔧 General Agent（通用任务 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `general` |
| **数量** | 0~N（可伸缩） |
| **生命周期** | 按需创建，任务完成后进入池待命 |
| **运行模式** | 非交互式（只接受任务、执行、返回） |

**职责：**
1. 执行 Orchestrator 派发的通用任务
2. 文件读取、代码搜索、代码修改
3. 工具调用的独立执行环境
4. 结果返回给 Orchestrator

**特点：**
- 无交互式 CLI（仅通过 IPC 通信）
- 有独立的 ContextManager（但只保留当前任务上下文）
- 可复用（任务完成后不清除，进入 Agent Pool）
- 支持超时取消

**使用场景：**
```
用户: "帮我同时调查 A、B、C 三个方向的可行性"
→ Orchestrator 派发 3 个 General Agent 并行执行
→ 各自完成任务后返回结果
→ Orchestrator 汇总给用户
```

---

### 4.4 ✅ Verifier Agent（验证 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `verifier` |
| **数量** | 0~3 |
| **生命周期** | 按需创建 |
| **运行模式** | 非交互式 |

**职责：**
1. **代码验证** — 修改代码后运行 `cargo check` / `cargo test`
2. **回归测试** — 运行预定义的测试套件
3. **端到端验证** — 运行场景测试
4. **编译优化** — 检测编译错误并分析根因
5. **质量门禁** — 决定代码修改是否通过质量检查

**特有工具：**
- `run_cargo_check(path)` — 运行编译检查
- `run_tests(test_filter)` — 运行测试
- `analyze_build_error(error_output)` — 分析编译错误

**与当前 spawn_agent 的关系：**
- 当前 spawn_agent = 编译 + 派生子进程执行任务（重、慢）
- Verifier Agent = 预编译、常驻、快速验证（轻、快）
- 当代码修改后，Orchestrator 通知 Verifier Agent 验证

---

### 4.5 📖 Reader Agent（阅读 Agent）— *可选*

| 属性 | 值 |
|------|-----|
| **角色名** | `reader` |
| **数量** | 0~N |
| **生命周期** | 按需创建 |
| **运行模式** | 非交互式 |

**职责：**
1. 专注阅读大型文件（Orchestrator 上下文放不下时）
2. 多文件关联分析
3. 代码结构理解与总结
4. 生成结构化分析报告

---

### 4.6 💻 Code Agent（编码 Agent）— *可选*

| 属性 | 值 |
|------|-----|
| **角色名** | `coder` |
| **数量** | 0~N |
| **生命周期** | 按需创建 |
| **运行模式** | 非交互式 |

**职责：**
1. 专注代码生成与修改
2. 多文件重构
3. 代码评审
4. 与 Verifier Agent 联动（修改→验证循环）

---

## 5. Agent 间通信模式

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

### 7.1 任务模型

```rust
/// 蜂群任务 — 可派发给任意 Agent 执行的工作单元
pub struct SwarmTask {
    pub task_id: String,
    pub task_type: String,          // 任务类型
    pub target_agent_type: AgentType, // 目标 Agent 类型
    pub payload: serde_json::Value, // 任务参数
    pub priority: TaskPriority,     // 优先级
    pub timeout: Duration,          // 超时
    pub max_retries: u32,           // 最大重试次数
    pub dependencies: Vec<String>,  // 依赖的任务 ID 列表
    pub created_at: chrono::DateTime<Utc>,
}

/// 任务执行结果
pub struct TaskResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub agent_id: String,
    pub started_at: chrono::DateTime<Utc>,
    pub completed_at: chrono::DateTime<Utc>,
}

pub enum TaskPriority {
    Low,
    Normal,
    High,
    Critical,
}
```

### 7.2 编排模式

#### 串行编排

```
[Task A] → [Task B] → [Task C]
```

当 Task A 完成后，自动派发 Task B。

#### 并行编排

```
        ┌── [Task B1]
[Task A] ── [Task B2]  →  [Task D] (汇合)
        └── [Task B3]
```

Task B1/B2/B3 并行执行，全部完成后派发 Task D。

#### 条件分支

```
              ┌── 成功 → [Task C]
[Task A] ────┤
              └── 失败 → [Task D] (补救)
```

根据 Task A 的结果决定下一步。

#### 循环迭代

```
[修改代码] → [验证] → ── 失败 → [修改代码] (循环)
                      └── 成功 → [完成]
```

Verifier Agent 返回失败时，重新派发修改任务。

### 7.3 编排 DSL（可选）

通过简单的 JSON 定义复杂 workflow：

```json
{
  "workflow": "implement_feature",
  "steps": [
    {
      "id": "analyze",
      "agent_type": "reader",
      "task": "分析需求文档",
      "next": "design"
    },
    {
      "id": "design",
      "agent_type": "general",
      "task": "设计实现方案",
      "next": "implement"
    },
    {
      "id": "implement",
      "agent_type": "coder",
      "task": "实现代码",
      "next": "verify"
    },
    {
      "id": "verify",
      "agent_type": "verifier",
      "task": "验证实现",
      "on_success": "complete",
      "on_failure": "implement"
    }
  ]
}
```

---

## 8. Agent 生命周期管理

### 8.1 状态机

```
         ┌──────────┐
         │  Starting │
         └─────┬─────┘
               │ 注册成功
               ▼
         ┌──────────┐
    ┌───│   Idle    │◄────────────┐
    │   └─────┬─────┘             │
    │         │ 派发任务           │
    │         ▼                   │
    │   ┌──────────┐              │
    │   │   Busy   │──────────────┘
    │   └─────┬─────┘  任务完成
    │         │ 超时/失败
    │         ▼
    │   ┌──────────┐
    │   │ Degraded │──────→ 重启
    │   └──────────┘
    │
    │   关闭
    └──→ ┌──────────┐
         │ Stopped  │
         └──────────┘
```

### 8.2 Agent 池管理

```
┌──────────────────────────────────────┐
│           Agent Pool                 │
│                                      │
│  通用 Agent 池 (max_pool_size=5)     │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐│
│  │ Idle│ │Idle│ │Busy│ │    │ │    ││
│  └────┘ └────┘ └────┘ └────┘ └────┘│
│                                      │
│  验证 Agent 池 (max_pool_size=2)    │
│  ┌────┐ ┌────┐                      │
│  │Idle │ │    │                      │
│  └────┘ └────┘                      │
└──────────────────────────────────────┘
```

**池策略：**
- 最小空闲数: 每种类型至少保留 N 个空闲 Agent
- 最大池大小: 防止资源耗尽
- 空闲回收: 空闲超过 T 分钟的 Agent 自动关闭
- 按需扩容: 当所有 Agent 忙碌时，创建新 Agent（不超过最大限制）

### 8.3 健康监控

```
Orchestrator:
  ├── 每 10s 检查所有 Agent 心跳
  ├── 连续 3 次心跳丢失 → 标记为 Failed
  ├── Failed Agent → 自动重启（最多 3 次）
  ├── 重启失败 → 发送告警给用户
  └── 记录 Agent 健康日志到文件
```

---

## 9. 实现阶段划分

### Phase 0 — 基础通信层（预计 1~2 天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 0.1 | 实现 UDS（Unix Domain Socket）Server/Client 框架 | `src/swarm/transport.rs` |
| 0.2 | 实现 JSON-RPC 2.0 协议解析与序列化 | `src/swarm/rpc.rs` |
| 0.3 | 实现 Agent 身份注册协议 | `src/swarm/registry.rs` |
| 0.4 | 实现心跳检测机制 | `src/swarm/heartbeat.rs` |
| 0.5 | 编写单元测试 + 集成测试 | 测试覆盖 |

**验证标准：**
- 两个进程间可通过 UDS 收发 JSON-RPC 消息
- Agent 启动后自动向 Orchestrator 注册
- 心跳超时自动触发重连

### Phase 1 — Swarm Registry（预计 1 天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 1.1 | 实现 `SwarmRegistry` 数据结构 | `src/swarm/registry.rs` |
| 1.2 | 实现 Agent 注册/注销/发现 API | 同上 |
| 1.3 | 实现 Agent 状态管理 | 同上 |
| 1.4 | 实现 `query_swarm` CLI 命令 | 命令行可查看蜂群状态 |

**验证标准：**
- 可以注册/注销 Agent
- 可按类型和状态查询 Agent
- `/swarm status` 命令可显示所有 Agent

### Phase 2 — Memory Agent（预计 2 天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 2.1 | 创建 `--agent-type memory` 启动模式 | `src/bin/memory_agent.rs` |
| 2.2 | 实现 Memory Agent 主循环（IPC 监听） | 同上 |
| 2.3 | 实现自动记忆提取逻辑 | `src/swarm/agents/memory.rs` |
| 2.4 | 实现记忆合并与去重 | 同上 |
| 2.5 | Orchestrator 集成：自动派发记忆任务 | `src/agent.rs` 修改 |

**验证标准：**
- Memory Agent 可独立启动并注册到 Orchestrator
- 每 5 轮对话后自动提取记忆
- 记忆可正确存储到向量数据库

### Phase 3 — General Agent（预计 2 天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 3.1 | 创建 `--agent-type general` 启动模式 | `src/bin/general_agent.rs` |
| 3.2 | 实现 General Agent 主循环（接受任务→执行→返回） | 同上 |
| 3.3 | 实现 Agent Pool 管理 | `src/swarm/pool.rs` |
| 3.4 | Orchestrator 集成：`dispatch_task` 工具 | 新工具注册 |

**验证标准：**
- General Agent 可接收任务并返回结果
- Agent Pool 可管理多个 General Agent
- 主 Agent 可通过 `/dispatch` 命令派发任务

### Phase 4 — Verifier Agent（预计 1 天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 4.1 | 创建 `--agent-type verifier` 启动模式 | `src/bin/verifier_agent.rs` |
| 4.2 | 实现编译验证和测试运行 | 同上 |
| 4.3 | 实现错误分析（解析编译错误信息） | 同上 |
| 4.4 | Orchestrator 集成：代码修改后自动派发验证 | `src/agent.rs` 修改 |

**验证标准：**
- Verifier Agent 可独立运行 `cargo check`
- 可返回详细的分析结果（错误位置、类型、建议）
- 修改代码后自动触发验证

### Phase 5 — 任务编排引擎（预计 2 天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 5.1 | 实现 Workflow 定义与解析 | `src/swarm/workflow.rs` |
| 5.2 | 实现串行/并行/条件分支执行 | 同上 |
| 5.3 | 实现循环迭代（修改→验证循环） | 同上 |
| 5.4 | Orchestrator 集成：workflow 执行 | `src/agent.rs` 修改 |

**验证标准：**
- 可定义和执行多步骤 workflow
- 支持并行任务执行
- 支持条件分支和循环

### Phase 6 — 优化与收尾（预计 1 天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 6.1 | 性能优化（连接池复用、消息压缩） | 各模块 |
| 6.2 | 错误处理完善（超时、重试、熔断） | `src/swarm/` |
| 6.3 | 文档更新 | `docs/designs/` |
| 6.4 | 集成测试（端到端场景） | `tests/` |

---

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

## 附录 A：与现有系统的集成点

### A.1 对 agent.rs 的修改

```rust
// 当前 Agent 结构体
pub struct Agent {
    // ... 现有字段 ...
    // 🆕 新增字段
    swarm_registry: Option<SwarmRegistry>,     // 蜂群注册表（Orchestrator 模式）
    swarm_client: Option<UdsClient>,            // 蜂群客户端（子 Agent 模式）
}

// Agent::run() 修改
impl Agent {
    pub async fn run(&mut self) -> anyhow::Result<()> {
        if self.is_orchestrator() {
            // 1. 启动 UDS 服务器（监听 Agent 注册）
            // 2. 启动 Memory Agent（自动）
            // 3. 启动原有的交互循环（增强）
            // 4. 处理 Agent 注册/心跳/任务结果
        } else {
            // 1. 注册到 Orchestrator
            // 2. 监听任务并执行
            // 3. 返回结果
        }
        Ok(())
    }
}
```

### A.2 对 ToolManager 的修改

```rust
// 新增工具
tool_manager.register_tool(Box::new(DispatchTask {
    swarm_registry: registry.clone(),
}));

tool_manager.register_tool(Box::new(QuerySwarm {
    swarm_registry: registry.clone(),
}));
```

### A.3 对模型管理的集成

每个 Agent 类型可以有不同的模型配置：

```rust
/// Agent 类型与模型映射
pub struct AgentModelMap {
    /// orchestrator → "deepseek"
    /// memory → "deepseek" (轻量模型)
    /// general → "deepseek"
    /// verifier → "deepseek" (快速模型)
    mappings: HashMap<AgentType, String>,
}
```

---

## 附录 B：示例场景

### 场景 1：并行调研

```
用户: "帮我调研 Rust 中 3 种异步运行时（tokio、async-std、smol）的优劣"

Orchestrator:
  ├── 派发 General Agent #1 → 调研 tokio
  ├── 派发 General Agent #2 → 调研 async-std
  ├── 派发 General Agent #3 → 调研 smol
  │
  ├── (等待所有结果)
  │
  └── 汇总结果给用户
```

### 场景 2：自动迭代修复

```
用户: "实现一个 CSV 解析器"

Orchestrator:
  ├── 派发 Coder Agent → 实现 CSV 解析器代码
  │
  ├── 代码完成后 → 派发 Verifier Agent
  │     ├── 运行 cargo check → 发现编译错误
  │     └── 返回错误详情
  │
  ├── 分析错误 → 派发 Coder Agent → 修复
  │
  ├── 再次派发 Verifier Agent → 验证通过
  │
  └── 完成，通知用户
```

### 场景 3：长期记忆自动维护

```
Memory Agent (后台运行):
  ├── 每 5 轮对话后:
  │     ├── 扫描最近上下文
  │     ├── 提取重要信息（如技术选型、架构决策）
  │     ├── 与已有记忆对比，去重
  │     └── 存储到向量数据库
  │
  ├── 每 50 轮对话后:
  │     ├── 执行记忆合并
  │     ├── 清理低价值记忆
  │     └── 更新记忆重要性评分
  │
  └── 压缩发生时:
        └── 自动检索相关记忆注入上下文
```

---

> **文档版本**: v1.0
> **更新日期**: 2025-06-08
> **作者**: Agent Lab 架构团队
> **审批状态**: 📋 待 review
