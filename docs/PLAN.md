

# 项目结构重构执行计划

> **目标**: 解决当前项目结构混乱问题，建立清晰一致的目录规范
> **状态**: 🔄 执行中
> **计划文档**: [ARCHITECTURE-REFACTORING-PLAN.md](./refactoring/ARCHITECTURE-REFACTORING-PLAN.md)

---

## 阶段 0：清理与准备

- [x] **0.1** 合并根目录和 docs/ 的状态文件
  - ✅ 合并内容到 docs/ 下对应文件
  - ✅ 更新 `src/task/mod.rs` 的 `STATE_FILES` 和文件路径（3处代码 + 2处测试）
  - ✅ 删除根目录的状态文件（PLAN.md, AGENDA.md, MEMORY.md）
  - ✅ 删除 `src/main.rs.bak` 和空 `src/renderer/` 目录
  - **验证**: cargo check ✅ | 94 tests passed ✅
- [x] **0.2** 删除无用文件（main.rs.bak, 空renderer目录） ✅

## 阶段 1：模块重命名

- [x] **1.1** 工具目录统一命名（去掉 _tool 后缀）
  - ✅ `base_shell/` → `shell/`
  - ✅ `edit_tool/` → `edit/`
  - ✅ `read_tool/` → `read/`
  - ✅ `search_tool/` → `search/`
  - ✅ `debug_tool/` → `tool_debug/`
  - ✅ 更新 `src/tools/mod.rs` 中的 mod 声明
  - ✅ 更新 `src/main.rs` 中的 import 路径
  - **验证**: cargo check ✅ | 94 tests passed ✅
- [x] **1.2** commands/ → cli/ 重命名
  - ✅ 目录重命名 `src/commands/` → `src/cli/`
  - ✅ 更新 `src/main.rs` 中的 `mod commands;` → `mod cli;`
  - ✅ 更新 `src/main.rs` 中的 import 路径
  - **验证**: cargo check ✅ | 94 tests passed ✅

> **阶段 2 具体计划**:
> - 2.1 context/mod.rs (1291行): 将 `#[cfg(test)] mod tests` (843行) 拆到 `src/context/tests.rs`
> - 2.2 session/mod.rs (613行): 将 `SerializableMessage` 类型拆到 `src/session/types.rs`，将 tests 拆到 `src/session/tests.rs`
>

## 阶段 2：大文件拆分

- [x] **2.1** 拆分 context/mod.rs（1291行）
  - ✅ 测试模块（843行）拆到 `src/context/tests.rs`
  - **验证**: cargo check ✅ | 94 tests passed ✅
- [x] **2.2** 拆分 session/mod.rs（613行）
  - ✅ 类型定义（127行）拆到 `src/session/types.rs`
  - ✅ 测试模块（120行）拆到 `src/session/tests.rs`
  - **验证**: cargo check ✅ | 94 tests passed ✅

## 阶段 3：文档重组

- [x] **3.1** 创建 docs/ 子目录结构，移动文档
- [x] **3.2** 创建 docs/index.md 文档索引

## 阶段 4：架构优化（可选）

- [x] **4.1** 创建 src/lib.rs 库入口
- [ ] **4.2** 从 main.rs 提取 Agent 核心循环到 agent.rs（待后续）

---

## 新任务：提取 Agent 主循环到 agent.rs + 多 Agent 支持

> **目标**: 将 main.rs 中的主循环提取到 agent.rs，支持多 Agent 并行运行
> **设计文档**: [MULTI_AGENT_ARCHITECTURE.md](./designs/MULTI_AGENT_ARCHITECTURE.md)

### 步骤

- [x] **1. 输出技术方案文档** → `docs/designs/MULTI_AGENT_ARCHITECTURE.md`
- [ ] **2. 实现 AgentConfig + AgentBuilder + Agent struct (agent.rs)**
  - AgentConfig 结构体（上下文策略配置）
  - AgentBuilder 构建器模式
  - Agent 结构体（持有所有状态）
  - 将 main.rs 的循环体移到 `Agent::run()`
  - 将辅助函数（session handling, render, etc.）移到 agent.rs
- [ ] **3. 实现 AgentHandle + spawn() 多 Agent 支持**
  - AgentHandle 结构体
  - `Agent::spawn()` 静态方法
- [ ] **4. 更新 lib.rs** → 添加 `pub mod agent;`
- [ ] **5. 精简 main.rs** → 只保留 CLI 参数解析和 Agent 创建
- [ ] **6. 验证** → cargo check + cargo test

---

## 新任务：DAG 任务编排系统实现

> **设计文档**: [dag-task-orchestration.md](./designs/dag-task-orchestration.md)
> **关键决策记录**: [MEMORY.md](./MEMORY.md#DAG)

### Phase 1 — 基础框架

- [x] **1.1** 创建 `src/dag/` 目录结构 + `src/lib.rs` 注册 ✅
- [x] **1.2** 实现核心数据模型（`PipelineDef`, `NodeDef`, `EdgeDef`, `NodeStatus`, 运行时类型） ✅
- [x] **1.3** 实现拓扑排序与环检测（Kahn 算法） ✅
- [x] **1.4** 创建 `src/dag/node_internal/` 子模块结构（预留 Phase 2 实现） ✅
- [x] **1.5** 编写 27 个单元测试覆盖基础逻辑 ✅
- [x] **验证**: `cargo check` ✅ | `cargo test` 121 passed ✅

### Phase 2 — 引擎与执行 ✅

- [x] **2.1** 实现 `DAGEngine` — 调度器主循环 ✅
- [x] **2.2** 实现 `DataFlowManager` — 数据传递与合并 ✅
- [x] **2.3** 实现 `WorkerAgent` 封装（基于 `DAGContext` + LLM 调用）✅
- [x] **2.4** 实现 `ReviewerAgent` 封装（JSON 审核结果解析 + 回退策略）✅
- [x] **2.5** 实现 `NodeSupervisor` — 节点内部协调（Worker→Reviewer→重试循环）✅
- [x] **验证**: `cargo check` ✅ | 27 DAG tests passed ✅

### Phase 3 — 工具与集成 ✅

- [x] **3.1** 创建 `dag_tools` 工具集（pipeline_build / pipeline_execute / pipeline_status / pipeline_list）✅
- [x] **3.2** 注册工具到 `ToolManager`（agent.rs 中 default_tool_manager）✅
- [ ] **3.3** AgentBuilder 扩展（worker_model / reviewer_model / pipeline_config）— 可选增强
- [x] **3.4** 注册 `pub mod dag;` 到 lib.rs ✅
- [x] **验证**: `cargo check` ✅

### Phase 4 — 增强打磨 ✅

- [x] **4.1** 断点续跑持久化 ✅
- [x] **4.2** 事件系统 ✅
- [x] **4.3** 审核重试策略优化（反馈注入重试提示）✅
- [x] **4.4** 可视化日志输出 ✅
## 实战测试：Agent 端到端使用效果验证

**目标**: 让子 Agent 执行一个真实的多步开发任务，验证实际使用效果

### 测试场景

子 agent 需要完成以下真实任务：

1. 读取项目中的 `.env` 文件和 `Cargo.toml`，了解项目配置
2. 读取 `src/dag/` 模块的代码结构
3. 在 `docs/` 下创建一个分析文档 `docs/analyses/dag-code-review.md`，分析 DAG 模块的代码质量
4. 整个过程中观察 agent 是否：能正确理解任务、能规划步骤、能使用工具、能输出结果

### 验证标准

- ✅ Agent 能自主规划并执行多步任务
- ✅ Agent 能正确使用 read/shell/edit 等工具
- ✅ Agent 能输出有实际价值的分析内容
- ✅ 整个过程无编译错误或崩溃

# DAG 系统与 Agent 核心集成 — 实现计划

## 目标
按照技术分析文档，将 DAG 系统接入完整的 Agent 主循环（带工具调用），并实现真正的并行执行。

## 步骤

- [x] Step 1: 改造 `runtime.rs` — 新增 `call_llm_with_tools()` 支持工具调用的 ReAct 循环；`DAGContext.tool_manager` 改为 `Arc<ToolManager>` 以支持共享
- [x] Step 2: 改造 `worker.rs` — Worker Agent 接入工具能力，使用 `call_llm_with_tools()`
- [x] Step 3: 改造 `supervisor.rs` — 传递 `tool_manager` 到 WorkerConfig（无需改动，ctx 已含 tool_manager）
- [x] Step 4: 重写 `execute.rs` — 实现真正的 DAG 并行执行（基于 DAGEngine 调度 + tokio::spawn）
- [x] Step 5: 改造 `engine.rs` — 补充 `on_node_failed()` 方法支持真实执行
- [x] Step 6: 编译验证 — `cargo check` ✅
- [x] Step 7: 功能验证 — 通过 spawn_agent 派生子 agent 测试 DAG 执行流程 ✅

---

## 新任务：Multi-Provider 模型接入 + /model 命令支持

> **目标**: 支持多个 LLM 提供商接入，支持通过 `/model` 命令运行时切换模型
> **设计文档**: [multi-provider-model.md](./designs/multi-provider-model.md)

### 步骤

- [x] **0. 输出技术方案** → `docs/designs/multi-provider-model.md`
- [ ] **1. 创建 ModelConfig 结构体** (src/model/config.rs)
- [ ] **2. 创建 ModelManager** (src/model/manager.rs) — 管理多模型注册与切换
- [ ] **3. 创建 providers 工厂函数** (src/model/providers.rs) — build_adapter()
- [ ] **4. 更新 model/mod.rs** 导出新模块
- [ ] **5. 修改 Agent 结构体集成 ModelManager** (agent.rs)
- [ ] **6. 在 cli/mod.rs 注册 /model 命令**
- [ ] **7. 在 agent.rs 主循环添加 /model 命令处理**
- [ ] **8. 更新 main.rs 使用 ModelManager 初始化**
- [ ] **9. cargo check 验证编译通过**

---

## 🐝 多 Agent 蜂群架构 — 端到端验证 & 收尾

> **目标**: 完成多 Agent 蜂群架构的所有剩余工作：编译修复 → 端到端验证 → 文档更新

### 步骤

- [x] **1. cargo check 编译验证** — 确保代码零错误零警告通过编译
- [x] **2. 运行全部测试** — 确保所有现有测试通过
- [x] **3. 端到端验证场景设计** — 设计多 Agent 蜂群的端到端测试场景
- [ ] **4. 端到端验证执行** — 🔄 正在执行...（用户已确认）
- [ ] **5. 文档与状态更新** — 更新 PLAN.md / AGENDA.md / MEMORY.md，标记所有阶段完成
- [ ] **6. 总结报告** — 向用户输出最终完成总结
