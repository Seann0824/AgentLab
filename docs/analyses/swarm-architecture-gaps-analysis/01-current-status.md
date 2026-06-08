# 🐝 多 Agent 蜂群架构—现状盘点：已建成的基础设施

> 原文拆分自 `../swarm-architecture-gaps-analysis.md`。

## 1. 已完成的 10 个模块

通过代码审查确认，以下基础设施已建设完成：

### 1.1 UDS 传输层 — `src/swarm/transport.rs` ✅ 完整

```
src/swarm/transport.rs (282 行)
├── UdsServer     (Orchestrator 使用 — accept/read/write/cleanup)
├── UdsClient     (Agent 使用 — connect/read/write/heartbeat)
├── UdsStream     (读写半连接封装)
└── UdsConnection (连接信息)
```

| 能力 | 状态 | 说明 |
|------|------|------|
| UnixListener 绑定 | ✅ | 自动创建目录、删除旧 socket |
| 结构化消息帧 | ✅ | 4 字节长度前缀 + JSON 消息体 |
| 注册握手协议 | ✅ | 连接后先发 register 消息 |
| 双向读写 | ✅ | `read_request()` / `write_response()` |
| 心跳发送 | ✅ | Agent 端 `send_heartbeat()` 支持 |
| 断开检测 | ✅ | read 返回 None 时的自动清理 |

### 1.2 JSON-RPC 协议 — `src/swarm/rpc.rs` ✅ 完整

```
src/swarm/rpc.rs (218 行)
├── JsonRpcRequest   (jsonrpc + id + method + params)
├── JsonRpcResponse  (jsonrpc + id + result | error)
├── JsonRpcError     (code + message + data)
└── SwarmMethod      (9 种预定义方法枚举)
```

**已定义的 9 种 RPC 方法：**

| 方法 | 方向 | 用途 |
|------|------|------|
| `register` | Agent → Orchestrator | Agent 注册 |
| `unregister` | Agent → Orchestrator | Agent 注销 |
| `heartbeat` | Agent → Orchestrator | 心跳维持 |
| `query_swarm` | Orchestrator → 自身 | 查询蜂群 |
| `dispatch_task` | Orchestrator → Agent | ⚠️ 已定义但**无调用方** |
| `cancel_task` | Orchestrator → Agent | ⚠️ 已定义但**无调用方** |
| `task_result` | Agent → Orchestrator | ⚠️ 已定义但**无调用方** |
| `event` | Agent → Orchestrator | 事件推送 |
| `broadcast` | Orchestrator → Agent | 广播通知 |

### 1.3 SwarmRegistry — `src/swarm/registry.rs` ✅ 完整

```
src/swarm/registry.rs (313 行)
├── AgentType        (7 种类型枚举)
├── AgentStatus      (Online/Busy/Offline/Failed)
├── AgentInfo        (完整注册信息)
└── SwarmRegistry    (注册/注销/查询/心跳/超时)
```

| 能力 | 状态 | 说明 |
|------|------|------|
| 注册 Agent | ✅ | agent_id + agent_type → AgentInfo |
| 注销 Agent | ✅ | 自动清理 |
| 按类型索引 | ✅ | type_index: HashMap<AgentType, Vec<id>> |
| 查询所有 | ✅ | 支持过滤类型 |
| 心跳更新 | ✅ | 更新 last_heartbeat 时间戳 |
| 超时检测 | ✅ | 30s 超时阈值，返回超时 ID 列表 |
| 在线统计 | ✅ | online_count / agent_count |

### 1.4 HeartbeatMonitor — `src/swarm/heartbeat.rs` ✅ 完整

- 启动后台 tokio::spawn 任务
- 每 10 秒检查一次 SwarmRegistry 中的超时 Agent
- 检测到超时时打日志
- 提供 `create_heartbeat_request()` 辅助函数

### 1.5 SwarmOrchestrator — `src/swarm/orchestrator.rs` ✅ 完整

```
src/swarm/orchestrator.rs (197 行)
├── bind()              → 创建 UDS 服务器 + 注册自己 + 启动心跳
├── accept_agent()      → 接受新连接并注册
├── start_accept_loop() → 后台无限接受循环
├── send_request()      → 向指定 Agent 发送 RPC 请求
├── broadcast()         → 向所有 Agent 广播
├── get_registry_snapshot() → 获取注册表快照
└── spawn_subprocess()  → 启动子进程 Agent
```

### 1.6 Agent Pool — `src/swarm/pool.rs` ✅ 完整

```
src/swarm/pool.rs (373 行)
├── AgentInstance        (id/type/status/client)
├── AgentPool            (空闲队列 + 忙碌列表 + 伸缩策略)
├── PoolAgentType        (General/Verifier/Custom)
└── AgentPoolManager     (管理多个 Pool)
```

| 能力 | 状态 | 说明 |
|------|------|------|
| 空闲队列管理 | ✅ | VecDeque 存储空闲实例 |
| 忙碌列表追踪 | ✅ | 正在执行任务的实例 |
| 获取/释放 | ✅ | acquire() / release() |
| 按需扩容 | ✅ | 全忙时创建新实例(不超 max) |
| 空闲超时回收 | ✅ | 5 分钟空闲自动关闭 |
| 初始化预创建 | ✅ | 启动时创建 min_size 个实例 |
| 统一管理器 | ✅ | AgentPoolManager 管理多池 |

### 1.7 Workflow Engine — `src/swarm/workflow/` ✅ 完整

```
src/swarm/workflow/ (共 500+ 行)
├── mod.rs       → 模块入口
├── types.rs     → Workflow / WorkflowStep / Condition / WorkflowState
├── engine.rs    → WorkflowEngine (拓扑排序 + 串/并行 + 条件 + 重试)
├── execution.rs → ⚠️ step 执行逻辑 (当前为 Mock!)
├── time.rs      → 时间格式化工具
└── tests.rs     → 单元测试
```

| 能力 | 状态 | 说明 |
|------|------|------|
| Workflow 定义 | ✅ | name + steps + timeout |
| 串行执行 | ✅ | 依赖拓扑排序 |
| 并行执行 | ✅ | 同层 step 并行 |
| 条件分支 | ✅ | OutputContains/OutputEquals/Success/Failure |
| 重试机制 | ✅ | 配置 retry_count |
| 步骤结果追踪 | ✅ | HashMap<step_id, StepResult> |
| 状态机 | ✅ | Pending→Running→Completed/Failed/Skipped |

### 1.8 5 种子 Agent 实现 — `src/swarm/agents/` ✅ 完整

| Agent | 行数 | 状态 | 特有工具 |
|-------|------|------|---------|
| MemoryAgent | 359 | ✅ | auto_extract, consolidate, forget |
| GeneralAgent | 189 | ✅ | 通用任务执行 |
| VerifierAgent | 263 | ✅ | run_cargo_check, run_tests, analyze_error |
| CoderAgent | 441 | ✅ | read_file, edit_file, generate_code, review_code |
| ResearcherAgent | 656 | ✅ | read_file, search_code, analyze_codebase, generate_report |

**共同能力：**

- 通过 UDS 连接到 Orchestrator
- 自动发送心跳
- `handle_request()` 方法接收并处理 RPC 请求
- `run()` 方法进入监听循环

### 1.9 CLI 入口 — `src/main.rs` ✅ 完整

```bash
agent-lab --agent-type orchestrator   # 交互式主 Agent
agent-lab --agent-type memory         # 记忆管理
agent-lab --agent-type general        # 通用任务
agent-lab --agent-type verifier       # 代码验证
agent-lab --agent-type coder          # 编码
agent-lab --agent-type researcher     # 技术调研
```

- 支持 `--socket-path` 和 `--orchestrator-socket` 参数
- Orchestrator 模式自动启动 Memory Agent 子进程
- 各 Agent 类型有完整的启动函数 (`run_*_agent`)

### 1.10 SwarmCtl 工具 — `src/tools/swarm_ctl.rs` ✅ 完整

- 已注册为 ToolManager 中的可用工具
- 支持 status / list / query 三种操作
- 渲染蜂群状态（在线 Agent 数、类型分布）

---

## 2. 现状总结

```
┌──────────────────────────────────────────────────────────────┐
│                    🐝 Agent Swarm (蜂群)                       │
│                                                              │
│  ✅ 通信层: UDS + JSON-RPC  ✅ 注册表: SwarmRegistry          │
│  ✅ 健康监控: Heartbeat      ✅ Orchestrator: 完整的 Server   │
│  ✅ Agent Pool: 完整的池化    ✅ Workflow: 完整的编排定义      │
│  ✅ 5 种 Agent 实现           ✅ CLI 入口                     │
│  ✅ SwarmCtl 工具             ✅ 自动启动 Memory Agent        │
│                                                              │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │  ⚠️ 但是：这些模块之间「没有连通」                        │ │
│  │  ⚠️ 整个系统像一个精美搭建但没通电的舞台                  │ │
│  └──────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```
