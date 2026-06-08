# MEMORY.md — 重要记录

> 历史记录已归档到 [MEMORY-history.md](archive/MEMORY-history.md)。本文件保留近期和仍需关注的项目记忆。

## DAG 任务编排系统 — 关键决策记录

### 架构设计决策

1. **Worker/Reviewer 不使用完整 Agent 实例**
   - 决策: WorkerAgent 和 ReviewerAgent 直接使用 `DAGContext`（持有 `Arc<Mutex<Box<dyn ModelAdapter>>>`），而非创建完整的 Agent 实例。
   - 理由: DAG 节点内部的 Worker/Reviewer 不需要完整的交互式 CLI 循环、上下文管理等重量级特性。轻量级 LLM 调用足够。
   - 影响: 执行效率更高，但 Worker 无法使用工具链（简化处理）。Phase 4 可增强支持工具回调。

2. **DAGContext 共享 ModelAdapter**
   - 决策: 使用 `Arc<Mutex<Box<dyn ModelAdapter>>>` 实现模型共享，所有 Worker/Reviewer 共用同一模型适配器。
   - 理由: 避免为每个节点创建新的模型连接，同时 `Arc<Mutex<>>` 确保线程安全。

3. **Reviewer 输出 JSON 格式**
   - 决策: Reviewer 要求 LLM 以结构化 JSON 格式输出审核结果（passed/score/feedback/check_results/suggestions）。
   - 理由: 方便程序化解析，同时兼容人类可读的回退策略（JSON 解析失败时自动宽松通过）。

4. **全局 Pipeline 存储**
   - 决策: 使用 `std::sync::LazyLock<Mutex<Vec<...>>>` 实现全局的 Pipeline 和 Engine 存储。
   - 理由: DAG 工具（pipeline_build/execute/status/list）需要跨 Agent 调用访问共享状态。

### 文件结构

```
src/tools/dag_tools/
├── mod.rs       — 模块导出
├── store.rs     — 全局 Pipeline/Engine 存储
├── build.rs     — pipeline_build 工具
├── execute.rs   — pipeline_execute 工具
├── status.rs    — pipeline_status 工具
└── list.rs      — pipeline_list 工具
```

### 待办

- Phase 3.3: AgentBuilder 扩展（worker_model / reviewer_model / pipeline_config）— 当前未实现，默认复用主 Agent 配置
- Phase 4: 审核反馈注入重试提示、持久化、事件系统

---

## DAG Phase 4 — 增强打磨（2025.??）

### 已实现的增强功能

#### 4.3 审核反馈注入重试（✅ 已完成）
- **WorkerConfig** 新增 `previous_feedback: Option<String>` 字段
- **WorkerAgent::execute** 构建系统提示时，如果有前次审核反馈，注入「前次审核反馈与修正要求」段落
- **NodeSupervisor::build_feedback_text** 将审核结果（评分、反馈、逐项检查、改进建议）格式化为结构化文本
- **NodeSupervisor::execute_with_retry** 维护反馈链（feedback_chain），每轮重试叠加历史反馈
- **修改文件**: `src/dag/node_internal/worker.rs`, `src/dag/node_internal/supervisor.rs`

#### 4.1 断点续跑持久化（✅ 已完成）
- **CheckpointManager** — 管理 DAGEngine 状态的 JSON 序列化/反序列化
- 保存到 `{base_dir}/{pipeline_id}/latest.json`（覆盖）和 `seq_{NNNN}.json`（历史）
- 支持 `save_checkpoint()`, `load_latest()`, `has_checkpoint()`, `list_checkpoints()`
- 新增 `serde::Serialize/Deserialize` 派生到核心类型（NodeDef, EdgeDef, PipelineDef, NodeStatus, DAGEvent 等）
- **新文件**: `src/dag/persistence.rs`

#### 4.2 事件系统增强（✅ 已完成）
- **EventBus** — 可克隆的事件发布/订阅总线，支持异步回调
- 支持文件日志（`create_event_logger` 将 DAGEvent 写入 JSON Lines 文件）
- **新文件**: `src/dag/event_bus.rs`

#### 4.4 可视化日志输出（✅ 已完成）
- **PipelineLogger** — ANSI 彩色输出到 stderr
- 支持：启动标题、节点状态变更（带颜色和 emoji）、完成摘要、进度条
- **log_engine_status()** — 快速输出 engine 当前状态摘要
- **新文件**: `src/dag/logger.rs`

#### 4.5 DAG 集成功能验证成功 ✅

**验证结果**:
- **pipeline_build**: ✅ 成功 — 正确解析 JSON 字段名 `id`（非 `pipeline_id`），节点用 `instruction`
- **pipeline_list**: ✅ 成功 — 列出所有已注册 Pipeline
- **pipeline_execute**: ✅ 成功 — Pipeline 执行完成，1/1 节点成功，0 失败
- **pipeline_status**: ✅ 成功 — 状态 `Completed`，耗时约 4.33 秒

**已应用的修复**:
- `src/tools/dag_tools/execute.rs`: 在 task spawn 前增加 `instance.transition_to(NodeStatus::Working)`，确保节点状态正确经过 `Working` → `Completed` 序列


---

## 🐝 多 Agent 蜂群架构 — 首次全面审查 (Phase 3.5)

**审查日期**: 当前

### 完成状态总结

| 组件 | 文件 | 状态 | 行数 |
|------|------|------|------|
| UDS Transport (Client/Server) | `src/swarm/transport.rs` | ✅ | ~300行 |
| JSON-RPC 协议 | `src/swarm/rpc.rs` | ✅ | ~100行 |
| Swarm Registry (AgentInfo/AgentStatus/AgentType) | `src/swarm/registry.rs` | ✅ | ~200行 |
| Heartbeat Monitor | `src/swarm/heartbeat.rs` | ✅ | ~100行 |
| Memory Agent | `src/swarm/agents/memory.rs` | ✅ 完整实现，4种记忆操作 + 心跳 |
| General Agent | `src/swarm/agents/general.rs` | ✅ 完整实现，带LLM主循环 |
| Verifier Agent | `src/swarm/agents/verifier.rs` | ✅ 完整实现，带LLM验证循环 |
| Agent Pool Manager | `src/swarm/pool.rs` | ✅ 350行，池化+按需创建+UDS |
| Swarm Orchestrator | `src/swarm/orchestrator.rs` | ✅ 189行，UDS Server+心跳+Agent管理 |
| Workflow Engine | `src/swarm/workflow/` | ✅ 已拆分为类型/引擎/执行器，拓扑排序+条件+并行+AgentPool集成 |
| main.rs 集成 (orchestrator/memory/general/verifier) | `src/main.rs` | ✅ 支持4种Agent类型启动 |
| tools/swarm_ctl 工具 | `src/tools/` | ✅ 已注册 (status/list/query) |

### 关键发现
1. **Phase 4 (Workflow Engine) 已完整实现** — 比原计划提前完成，支持拓扑排序、条件分支、并行执行
2. **所有代码编译通过** — cargo check 零错误
3. **138个测试全部通过** — cargo test 零失败
4. **spawn_agent工具可用** — 可以用于端到端验证

### 待完成项
- ⬜ **Phase 3.5 — 端到端验证**: 需要手动或派生子Agent验证蜂群上下文的完整流程
- ⬜ **Phase 5 — 文档完善**: 更新最终文档和总结

### 端到端验证场景设计
1. **基础编译验证** ✅ — cargo check + cargo test 已通过
2. **蜂群启动流程验证**: 启动 Orchestrator → 自动启动 Memory Agent → 注册 → 心跳
3. **Agent Pool 验证**: AgentPool 初始化 → 分配 → 回收
4. **Workflow 执行验证**: 定义 Workflow → 执行 → 查看状态
