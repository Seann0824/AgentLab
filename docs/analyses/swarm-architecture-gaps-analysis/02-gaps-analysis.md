# 🐝 多 Agent 蜂群架构—六项关键缺口分析

> 原文拆分自 `../swarm-architecture-gaps-analysis.md`。

## 1. 缺口全景

```
┌─────────────────────────────────────────────────────────────┐
│                    已建成 (10 模块)                          │
│  ┌─────────┐ ┌────────────┐ ┌──────────┐ ┌──────────────┐ │
│  │ UDS 传输│ │ JSON-RPC   │ │ Registry │ │ Orchestrator │ │
│  └────┬────┘ └─────┬──────┘ └────┬─────┘ └──────┬───────┘ │
│       │            │             │              │          │
│       └────────────┼─────────────┼──────────────┘          │
│                    │    ❌ 没有连通!                        │
│  ┌─────────┐ ┌────▼──────┐ ┌────▼─────┐ ┌──────────────┐ │
│  │ 5 Agents │ │ Pool      │ │ Workflow │ │ SwarmCtl     │ │
│  └─────────┘ └───────────┘ └──────────┘ └──────────────┘ │
└─────────────────────────────────────────────────────────────┘

                    🔴 6 项关键缺口
    ┌──────┬──────┬──────┬──────┬──────┬──────┐
    │ #1   │ #2   │ #3   │ #4   │ #5   │ #6   │
    │任务  │派发  │Work- │Pool+ │任务  │错误  │
    │模型  │工具  │flow  │Orch  │回传  │恢复  │
    │缺失  │缺失  │Mock  │孤立  │缺失  │缺失  │
    └──────┴──────┴──────┴──────┴──────┴──────┘
```

---

## 2. 缺口 #1 (🔴 致命): 无任务模型 — SwarmTask/TaskResult 缺失

### 2.1 现状

设计文档定义了完整的任务模型，但代码中不存在：

| 结构体 | 设计文档 | 代码中 | 影响 |
|--------|---------|--------|------|
| `SwarmTask` | ✅ 完整定义 | ❌ 不存在 | 无法表示一个可派发的任务 |
| `TaskResult` | ✅ 完整定义 | ❌ 不存在 | 无法表示任务执行结果 |
| `TaskPriority` | ✅ Low/Normal/High/Critical | ❌ 不存在 | 无法做优先级调度 |
| `TaskStatus` | ✅ Pending/Running/Completed/Failed | ❌ 不存在 | 无任务生命周期追踪 |

### 2.2 影响范围

```
缺失 SwarmTask 的连锁反应:
  ├── dispatch_task 工具无法定义参数类型
  ├── Orchestrator 无法持久化任务状态
  ├── WorkflowEngine 无法传递结构化任务描述
  ├── Agent Pool 无法关联实例与任务
  └── 用户无法查看任务执行进度
```

### 2.3 修复优先级

**🔴 P0 — 必须先于所有其他修复完成**，因为它是任务派发的基础数据结构。

---

## 3. 缺口 #2 (🔴 致命): 无 dispatch_task 工具

### 3.1 现状

```
Orchestrator 持有：
  ✓ 已连接的 Agent 流 (streams: HashMap<String, UdsStream>)
  ✓ 注册表 (SwarmRegistry)
  ✓ send_request() 方法

但 Orchestrator 没有任何工具可以向 Agent 派发任务！
```

`SwarmOrchestrator::send_request()` 已经实现了发送 RPC 请求到指定 Agent 的能力，但：

1. **Orchestrator 的 Agent（主循环）中没有任何途径调用它**
2. **没有 `dispatch_task` 工具注册到 ToolManager**
3. **没有从 LLM 到 send_request 的调用链路**

### 3.2 对比旧 spawn_agent 工具

| 对比维度 | 旧 spawn_agent | 期望的 dispatch_task |
|----------|---------------|---------------------|
| 编译 | 每次 `cargo build` (~30s) | 零编译（Agent 已常驻） |
| 生命周期 | 执行完即销毁 | Agent 持续运行，可复用 |
| 上下文 | 全新（无历史） | 可选择性保留 |
| 通信方式 | stdin/stdout 文本 | 结构化 JSON-RPC |
| 并发 | 串行（一次一个） | 并行派发多个 Agent |
| 资源开销 | 每个 Agent 一个进程+编译 | 池化复用 |

### 3.3 根因

`src/tools/` 目录下 **没有** `dispatch_task.rs` 文件：

```
src/tools/
├── mod.rs           → 注册所有工具
├── investigate.rs   → 错误快照分析
├── swarm_ctl.rs     → 蜂群控制
├── subagent/        → 旧 spawn_agent（仍用编译+子进程）
├── generate_tool/   → 工具脚手架生成
└── ...              → dispatch_task.rs 不存在
```

### 3.4 修复优先级

**🔴 P0 — 与 #1 并列最高优先级**，连接 Orchestrator 与子 Agent 的核心枢纽。

---

## 4. 缺口 #3 (🟡 中): Workflow Engine 执行是 Mock

### 4.1 现状

```rust
// src/swarm/workflow/execution.rs (第 31 行)
pub(super) async fn execute_step(
    pool_manager: Arc<TokioMutex<AgentPoolManager>>,
    _task: &str,
    step_name: String,
) -> Result<String> {
    // ... 从 Pool 获取实例 ...
    
    // ⚠️ 模拟执行（真实场景中通过 UDS 发送任务给 Agent）
    tokio::time::sleep(Duration::from_millis(200)).await;  // ← 纯 Mock!
    let result = format!("步骤 '{}' 执行完成，使用了实例 '{}'", step_name, instance_name);
    
    // 释放实例回池
    // ...
    Ok(result)
}
```

### 4.2 影响

```
WorkflowEngine 当前状态:
  ├── ✅ 拓扑排序正确
  ├── ✅ 串/并行逻辑正确
  ├── ✅ 条件分支判断正确
  ├── ✅ 重试+超时逻辑正确
  └── ❌ 实际步骤执行是 sleep(200ms) 返回假结果
  
→ 整个 Workflow 功能无法用于实际任务
→ 只能用于单元测试验证编排逻辑
```

### 4.3 修复优先级

**🟡 P1 — 依赖 #1 和 #2 完成后才能修复**。需要 `dispatch_task` 工具就绪后，将 `execute_step` 中的 Mock 替换为真正的 UDS 任务派发。

---

## 5. 缺口 #4 (🟡 中): Agent Pool 与 Orchestrator 孤立

### 5.1 现状

```
目前有两个独立的 Agent 管理系统：

┌──────────────────────┐     ┌──────────────────────┐
│   SwarmOrchestrator   │     │   AgentPoolManager    │
│                       │     │                       │
│   streams: HashMap    │     │   general_pool        │
│   registry: Registry  │     │   verifier_pool       │
│                       │     │                       │
│   功能: 接受连接       │     │   功能: 管理实例池     │
│   发送消息             │     │   获取/释放           │
│   查询注册表           │     │   伸缩/回收           │
│                       │     │                       │
│   ❌ 不知道 Pool 存在  │     │   ❌ 不知道   │
│                       │     │       Orchestrator    │
└──────────────────────┘     └──────────────────────┘

问题:
1. Agent Pool 创建的实例不会自动注册到 Orchestrator
2. Orchestrator 不知道哪些 Agent 属于哪个 Pool
3. Pool 中的实例连接到了 Orchestrator，但 Orchestrator 的 streams map 中没有它们
4. 任务派发时不知应该从 Pool 获取还是直接使用 streams
```

### 5.2 具体代码路径

```rust
// src/swarm/pool.rs — Pool 创建实例
async fn spawn_instance(&self, index: usize) -> Result<AgentInstance> {
    // 创建 UdsClient 并连接
    let client = UdsClient::connect(&self.orchestrator_socket, &agent_id).await?;
    // ...
    // ⚠️ 连接后会注册到 Orchestrator，但 Orchestrator 的 accept_loop
    //    只在 server.accept() 时注册，这里是用 client 主动连接
    //    → 两者的注册路径完全不同！
    //    → Pool 创建的 Agent 不会被 Orchestrator 的 streams 追踪
}
```

### 5.3 修复优先级

**🟡 P1 — 与 #3 同级**，需要引入 `SwarmTask` 模型并解决路由问题。

---

## 6. 缺口 #5 (🟠 高): 无 Agent → Orchestrator 任务回传

### 6.1 现状

```
Agent 端 (子进程) 已经实现了完整的接收-执行-返回逻辑：

GeneralAgent::handle_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
    match request.method.as_str() {
        "dispatch_task" => {
            // ✅ 解析任务
            // ✅ 执行任务（使用本地 ToolManager）
            // ✅ 返回 JsonRpcResponse::success(id, result)
        }
        // ...
    }
}

但是 Orchestrator 端:
  ❌ 注册了 dispatch_task 的 RPC 方法名
  ❌ 但没有处理 task_result 回传的代码路径
  ❌ accept_loop 只接受连接，不读取子 Agent 的响应
  ❌ 没有等待任务完成的异步机制
```

### 6.2 影响

```
任务派发流程堵塞在半路：
  Orchestrator → send_request("dispatch_task", ...) → Agent
  Agent → 执行完成 → 返回结果 → ❌ Orchestrator 没有在读！
```

### 6.3 修复优先级

**🟠 P1 — 需要与 #2 一并设计**。`dispatch_task` 工具不仅要能发送任务，还要能接收并返回结果。

---

## 7. 缺口 #6 (🔵 低): 缺少错误恢复与容错

### 7.1 现状

| 容错场景 | 当前行为 | 期望行为 |
|---------|---------|---------|
| Agent 心跳超时 | 仅打日志 | 自动重启子进程 |
| Agent 进程崩溃 | 注册表仍显示 Online | 检测到断开后标记 Offline |
| Workflow 步骤失败 | 返回 Failed 状态 | 执行降级策略（替代 Agent/重试）|
| UDS 连接断开 | 流被移除 | 自动重连 |
| Orchestrator 重启 | 子 Agent 全部断开 | 子 Agent 自动重连 |

### 7.2 修复优先级

**🔵 P2 — 低优先级**，在核心流程连通后再考虑。当前核心问题是"功能缺失"，不是"稳定性缺失"。

---

## 8. 缺口优先级总结

| 优先级 | 缺口 | 依赖 | 预估工作量 |
|--------|------|------|-----------|
| 🔴 P0 | #1 SwarmTask 模型 | 无 | ~100 行（纯数据结构） |
| 🔴 P0 | #2 dispatch_task 工具 | #1 | ~300 行（工具 + 调用链路） |
| 🟠 P1 | #5 任务回传 | #2 | ~150 行（响应处理） |
| 🟡 P1 | #3 Workflow 真实执行 | #1, #2 | ~100 行（替换 Mock） |
| 🟡 P1 | #4 Pool + Orchestrator 整合 | #1, #2 | ~200 行（注册+路由） |
| 🔵 P2 | #6 错误恢复 | 以上全部 | ~300 行 |

**推荐执行顺序：** #1 → #2 → #5 → #3 + #4（可并行） → #6
