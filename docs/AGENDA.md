# 📋 当前议程 — 项目结构重构

**任务名称**: 提取 Agent 循环 + 多 Agent 支持
**进度**: 🔄 阶段 1 / 3 阶段
**当前步骤**: 1. 输出技术方案文档到 docs/

---

## 完成状态

| 阶段 | 状态 |
|------|------|
| 阶段 0：清理与准备 | ✅ 已完成 |
| 阶段 1：模块重命名 | ✅ 已完成 |
| 阶段 2：大文件拆分 | ✅ 已完成 |
| 阶段 3：文档重组 | ✅ 已完成 |
| 阶段 4：架构优化 | ✅ 已完成（4.1 完成，4.2 待后续） |

---

## 新任务：提取 Agent 循环 + 多 Agent 支持

| 步骤 | 状态 |
|------|------|
| 1. 输出技术方案文档到 docs/designs | ✅ 已完成 |
| 2. 实现 AgentConfig + AgentBuilder + Agent struct (agent.rs) | 🔄 执行中 |
| 3. 实现 AgentHandle + spawn() 多 Agent 支持 | ⬜ |
| 4. 更新 lib.rs 导出 agent 模块 | ⬜ |
| 5. 精简 main.rs 为薄壳 | ⬜ |
| 6. 验证：cargo check + cargo test | ⬜ |

## 🔧 修复编译错误（SpawnAgent Send + borrow after move）

| 步骤 | 状态 |
|------|------|
| 1. 修复 `ChatMessage` 非 Send（ToolResultRenderer + Send bound） | ✅ 已完成 |
| 2. 修复 `SpawnAgent` 的 model 字段类型为 Arc<Mutex<...>> | ✅ 已完成 |
| 3. 修复 `AgentBuilder::build()` borrow after move | ✅ 已完成 |
| 4. 验证：cargo check | ✅ 已完成 |


---

## DAG 任务编排系统

**任务名称**: DAG 任务编排系统实现  
**进度**: ✅ Phase 1 / 4 完成  
**当前步骤**: 2.1 实现 DAGEngine 调度器主循环

| Phase | 状态 |
|-------|------|
| Phase 1：基础框架 | ✅ 已完成（27测试通过） |
| Phase 2：引擎与执行 | 🔄 执行中 |
| Phase 3：工具与集成 | ⬜ |
| Phase 4：增强打磨 | ⬜ |

---

## 当前任务：DAG 任务编排系统 — Phase 2&3 实现

**进度**: 95%
**当前步骤**: Phase 2（引擎与执行）✅ 完成 | Phase 3（工具与集成）✅ 完成（3.3 可选）
**下一步**: Phase 4 — 增强打磨

### 完成内容
- Phase 2: WorkerAgent / ReviewerAgent / NodeSupervisor / NodeRuntime 实现
- Phase 3: dag_tools 工具集（build / execute / status / list）+ 注册到 ToolManager


## Phase 4: DAG 增强打磨 ✅

**进度**: 100%
**当前步骤**: 全部完成 ✅

### 执行计划（全部完成 ✅）
1. ✅ **4.3** 审核重试策略优化 — WorkerAgent 支持接收 Reviewer 反馈，重试时传递
2. ✅ **4.1** 断点续跑持久化 — JSON 序列化 checkpoint，支持恢复执行
3. ✅ **4.2** 事件系统增强 — EventBus 发布/订阅/日志
4. ✅ **4.4** 可视化日志输出 — ANSI 彩色输出 Pipeline 执行过程
