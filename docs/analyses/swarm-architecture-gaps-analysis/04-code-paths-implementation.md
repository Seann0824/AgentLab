# 🐝 多 Agent 蜂群架构—关键代码路径与实现方案

> 原文拆分自 `../swarm-architecture-gaps-analysis.md`。

## 1. 核心调用链路图

### 1.1 当前（断裂的）调用链路

```
LLM (Orchestrator 主循环)
  │
  ├── 调用 spawn_agent 工具
  │     └── cargo build (30s) → 启动子进程 → stdin/stdout 文本交互
  │
  └── 调用 SwarmCtl 工具（只读查询）
        └── 查询 SwarmRegistry → 显示 Agent 列表

  ❌ 没有任何方式可以调用 Memory/General/Verifier/Coder/Researcher Agent
```

### 1.2 期望的调用链路（改造后）

```
LLM (Orchestrator 主循环)
  │
  ├── 调用 dispatch_task 工具 ← 新工具！
  │     │
  │     ├── DispatchTask::execute()
  │     │     ├── 1. 解析参数 → SwarmTask
  │     │     ├── 2. 通过 Registry 查找目标 Agent
  │     │     ├── 3. 创建 oneshot channel
  │     │     ├── 4. 存入 pending_requests
  │     │     ├── 5. orch.send_request(agent_id, swarm_task)
  │     │     │      └── UDS → Agent stream → Agent::handle_request()
  │     │     │             └── Agent 执行任务 → 返回 JsonRpcResponse
  │     │     ├── 6. 后台 reader_loop 收到响应
  │     │     ├── 7. 通过 oneshot 唤醒调用方
  │     │     └── 8. 返回结果给 LLM
  │     │
  │     └── 结果: ✅ 完整的「派发→执行→回传」链路
  │
  ├── 调用 spawn_agent 工具（保留，用于隔离验证）
  │
  └── 调用 SwarmCtl 工具（增强，显示任务统计）
```

---

## 2. 关键代码路径详解

### 2.1 路径 A：dispatch_task 派发流程

```
时序图：

LLM                      DispatchTask              SwarmOrchestrator              Agent
 │                            │                          │                        │
 │  1. 调用工具                │                          │                        │
 │ ──────────────────────────►│                          │                        │
 │                            │                          │                        │
 │  2. find_idle_agent()       │                          │                        │
 │   ─────────────────────────► Registry                  │                        │
 │   ◄───────────────────────── agent_id                  │                        │
 │                            │                          │                        │
 │  3. oneshot::channel()      │                          │                        │
 │   ──── 创建 tx/rx ─────────►                          │                        │
 │                            │                          │                        │
 │  4. send_request()          │                          │                        │
 │   ─────────────────────────► orch ─── UDS ────────────► agent                   │
 │                            │                          │                        │
 │  5. await rx                │    6. handle_request()   │                        │
 │   (阻塞等待)                 │     ◄────────────────────┘                        │
 │                            │                          │                        │
 │                            │    7. 执行任务            │                        │
 │                            │     (ToolManager)         │                        │
 │                            │     ├── read/search etc. │                        │
 │                            │     └── 聚合结果          │                        │
 │                            │                          │                        │
 │                            │    8. write_response()    │                        │
 │                            │   ◄────── UDS ────────────┘                        │
 │                            │                          │                        │
 │  9. 收到响应                │                          │                        │
 │  ◄──────────────────────────┘                          │                        │
 │                            │                          │                        │
 │ 10. 结果返回给用户          │                          │                        │
```

### 2.2 路径 B：Workflow 执行流程

```
WorkflowEngine                    dispatch_task              Agent Pool
     │                                │                        │
     │ 1. execute(workflow)            │                        │
     │    ├── 拓扑排序                  │                        │
     │    ├── 逐组执行 (串/并行)         │                        │
     │    │                            │                        │
     │    ├── 步骤 1: "调研方案 A"       │                        │
     │    │   ├── pool.acquire() ──────►──────────────────────► │
     │    │   │                        │    返回 AgentInstance  │
     │    │   ◄────────────────────────┼────────────────────────│
     │    │   │                        │                        │
     │    │   └── dispatch_task() ────►│                        │
     │    │         (通过 Orch 发送)    │                        │
     │    │                            │                        │
     │    ├── 步骤 2: "调研方案 B" (并行)│                        │
     │    │   └── 同上                  │                        │
     │    │                            │                        │
     │    ├── 等待所有并行步骤完成        │                        │
     │    ├── 释放实例回池              │                        │
     │    └── 返回 WorkflowState        │                        │
```

### 2.3 路径 C：Agent 内部处理流程

```rust
// 所有 Agent 共享的处理模式
impl GeneralAgent {
    pub async fn handle_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        match request.method.as_str() {
            "dispatch_task" => {
                // 1. 解析任务参数
                let params = request.params.unwrap_or_default();
                let task_desc = params["task_description"].as_str().unwrap_or("");
                let timeout = params["timeout_seconds"].as_u64().unwrap_or(60);
                
                // 2. 创建本地 ToolManager（或使用内置的执行器）
                let result = self.execute_task(task_desc, timeout).await?;
                
                // 3. 返回成功响应
                Ok(JsonRpcResponse::success(
                    request.id,
                    serde_json::json!({ "result": result }),
                ))
            }
            "heartbeat" => {
                Ok(JsonRpcResponse::success(request.id, serde_json::json!({ "status": "ok" })))
            }
            _ => {
                Ok(JsonRpcResponse::error(
                    request.id, -32601, format!("Method '{}' not found", request.method),
                ))
            }
        }
    }
}
```

---

## 3. 需要修改的文件清单

### 阶段 1（P0）：**~8 个文件**

| 操作 | 文件 | 修改内容 |
|------|------|---------|
| ✅ 新建 | `src/swarm/task.rs` | SwarmTask + TaskResult + TaskStatus + TaskPriority |
| ✅ 编辑 | `src/swarm/mod.rs` | 新增 `pub mod task;` + 重新导出 |
| ✅ 新建 | `src/tools/dispatch_task.rs` | DispatchTask 工具实现 |
| ✅ 编辑 | `src/tools/mod.rs` | 注册 DispatchTask |
| ✅ 编辑 | `src/swarm/orchestrator.rs` | 新增 pending_requests + reader_loop |
| ✅ 编辑 | `src/agent/default_tools.rs` | 将 DispatchTask 注册到 ToolManager |
| ✅ 可选 | `src/agent/swarm_command.rs` | 增强 /swarm 命令 |
| ✅ 可选 | `src/main.rs` | 传递 orch_arc 给 DispatchTask |

### 阶段 2（P1）：**~5 个文件**

| 操作 | 文件 | 修改内容 |
|------|------|---------|
| ✅ 编辑 | `src/swarm/workflow/execution.rs` | 替换 Mock → 真实 UDS 派发 |
| ✅ 编辑 | `src/swarm/workflow/engine.rs` | 注入 Orchestrator 引用 |
| ✅ 编辑 | `src/swarm/pool.rs` | Pool 实例自动注册到 Registry |
| ✅ 编辑 | `src/swarm/orchestrator.rs` | 路由策略（Pool → Registry）|
| ✅ 编辑 | `src/main.rs` | 传递更多依赖 |

### 阶段 3（P2）：**~6 个文件**

| 操作 | 文件 | 修改内容 |
|------|------|---------|
| ✅ 编辑 | `src/swarm/agents/*.rs` | 所有 Agent 添加断线重连 |
| ✅ 编辑 | `src/swarm/orchestrator.rs` | 心跳超时后自动重启 |
| ✅ 编辑 | `src/swarm/heartbeat.rs` | 增强超时处理逻辑 |
| ✅ 编辑 | `src/tools/swarm_ctl.rs` | 显示任务统计+健康状态 |
| ✅ 新建 | `src/swarm/monitor.rs` | 事件日志/健康检查 API |

---

## 4. 风险与注意事项

### 4.1 并发安全

```
DispatchTask 在 tokio::spawn 中执行：
  └── 需要能安全访问 SwarmOrchestrator 的 streams 和 pending_requests
  └── 方案: 使用 Arc<Mutex<SwarmOrchestrator>> 共享引用
  └── 注意: 避免死锁！send_request 时不能同时持有 orch 锁
```

### 4.2 超时处理

```
dispatch_task 的场景：
  1. Agent 连接断开 → 返回 ConnectionError
  2. Agent 执行超时 → 返回 TimeoutError
  3. Agent 执行失败 → 返回 TaskFailed + 错误信息
  4. 正常返回 → 返回 TaskResult

所有超时必须有兜底，不能让 LLM 无限等待。
```

### 4.3 向后兼容

```
1. 旧 spawn_agent 保留不动（不删除，不影响已有功能）
2. SwarmRegistry 的 Snapshot 保持现有格式
3. Agent 的 handle_request 兼容原有 RPC 方法名
4. 新的 SwarmTask 不修改已有的 task::TaskManager 代码
```

### 4.4 测试策略

```
1. 单元测试: SwarmTask 序列化/反序列化
2. 集成测试: dispatch_task 工具在模拟 Stream 上执行
3. 端到端测试: 启动真实 Agent 进程，派发真实任务
4. Workflow 测试: 用 Mock Agent 验证编排逻辑，用真实 Agent 验证执行
```

---

## 5. 最终架构状态

```
阶段 3 完成后，蜂群架构将变为：

┌──────────────────────────────────────────────────────────────┐
│                    🐝 Agent Swarm (蜂群)                       │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │          🧠 Orchestrator Agent (调度者)                  │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────────────────┐   │  │
│  │  │  LLM     │ │ ToolMgr  │ │ Task Tracker         │   │  │
│  │  │  主循环   │ │ dispatch │ │ SwarmTask + History  │   │  │
│  │  └──────────┘ └──────────┘ └──────────────────────┘   │  │
│  │  ┌────────────────────────────────────────────────┐   │  │
│  │  │ SwarmOrchestrator                               │   │  │
│  │  │  ├── UDS Server → accept_loop (接受连接)         │   │  │
│  │  │  ├── pending_requests → reader_loop (读响应)     │   │  │
│  │  │  ├── Registry → Agent 注册/发现/心跳             │   │  │
│  │  │  └── PoolMgr → 实例池+伸缩+回收                   │   │  │
│  │  └────────────────────────────────────────────────┘   │  │
│  └──────────────────────┬───────────────────────────────┘  │
│                         │ UDS                               │
│    ┌────────────────────┼────────────────────┐              │
│    │                    │                    │              │
│ ┌──▼─────┐  ┌──────────▼──┐  ┌──────────────▼─────┐      │
│ │ 🧠     │  │ 🔧 General  │  │ ✅ Verifier        │      │
│ │ Memory │  │ Agent Pool  │  │ Agent Pool         │      │
│ │ Agent  │  │ (0~5 实例)   │  │ (0~3 实例)          │      │
│ └────────┘  └─────────────┘  └────────────────────┘      │
│    ┌───────────┐  ┌──────────────┐                        │
│    │ 💻 Coder  │  │ 🔬 Researcher│                        │
│    │ Agent     │  │ Agent        │                        │
│    └───────────┘  └──────────────┘                        │
│                                                              │
│  所有 Agent: ✅ 常驻进程 ✅ 自动注册 ✅ 心跳维持              │
│             ✅ 断线重连 ✅ 接受任务 ✅ 返回结果                │
└──────────────────────────────────────────────────────────────┘
```
