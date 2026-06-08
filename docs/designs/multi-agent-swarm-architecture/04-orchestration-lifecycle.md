# 🐝 多 Agent 蜂群架构设计（Multi-Agent Swarm Architecture） — 任务编排、生命周期与阶段计划

> 原文拆分自 `../multi-agent-swarm-architecture.md`。


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

