# DAG Pipeline 可观测性改进计划

## 目标
让 DAG Pipeline 的执行过程完全可观测（observable），解决"执行后无法观测"的核心痛点。

## 步骤

1. ✅ **[可观测] pipeline_execute 返回详细节点结果**：包含每个节点的 worker_output、review_result、final_output
2. ✅ **[可观测] pipeline_status 显示完整节点细节**：从保存的引擎中提取每个节点的详细状态和输出
3. ✅ **[可观测] 实时 stderr 日志**：执行过程中输出节点进度、Worker 摘要、Reviewer 决策到 stderr
4. ✅ **[可验证] 编译验证**：cargo check 通过（仅 warnings，无 errors）
# 🗺️ 并发完成：多 Agent 协作(P2) + 持久化会话(P3)

## 目标
验证并完善 DAG Pipeline 多 Agent 并行执行能力，同时验证会话持久化能力，使两者达到可用状态。

---

## Track A: 多 Agent 协作 — DAG Pipeline 端到端验证

- [x] **A1 验证 DAG 上下文初始化** — Agent 启动时调用 `init_dag_context`，包含 model 和 tool_manager ✅
- [x] **A2 构建并注册示例 Pipeline** — 使用 `pipeline_build` 创建了 `demo-parallel-pipeline`，4节点，step2a/step2b 可并行 ✅
- [x] **A4 验证 pipeline_list** — `pipeline_list` 返回 Pipeline 信息 ✅
- [ ] **A3 执行 Pipeline 验证并行执行** — 使用 `pipeline_execute` 运行，确认无依赖节点并行执行
- [ ] **A4 验证 pipeline_status / pipeline_list** — 查询执行状态和已注册 Pipeline

## Track B: 持久化会话 — Session 能力端到端验证

- [x] **B1 验证 /session 命令可用** — CLI 中 `/session` 在 agent.rs:429 已完整接入，含 save/load/list/delete/rename ✅
- [ ] **B2 测试 /session save** — 保存当前对话
- [ ] **B3 测试 /session list** — 列出所有已保存会话
- [ ] **B4 更新文档状态** — 更新 README.md 反映实际完成状态

## 验证标准
- DAG: Pipeline 注册成功，执行后节点状态正确
- Session: 保存/加载/列出会话无错误
- 代码编译无错误



# 🗺️（最新任务）并发完成：多 Agent 协作(P2) + 持久化会话(P3)

## 目标
验证并完善 DAG Pipeline 多 Agent 并行执行能力，同时验证会话持久化能力。

---

## Track A: 多 Agent 协作 — DAG Pipeline 端到端验证

- [x] **A1 DAGContext 初始化** ✅
- [x] **A2 pipeline_build 创建示例 Pipeline** ✅ — `demo-parallel-pipeline`，4节点，step2a/step2b 可并行
- [x] **A4 pipeline_list** ✅ — 列出已注册 Pipeline
- [ ] **A3 pipeline_execute** — 执行 Pipeline 验证并行执行
- [ ] **A4 pipeline_status** — 查询执行后状态

## Track B: 持久化会话 — Session 能力端到端验证

- [x] **B1 /session 命令可用** ✅ — agent.rs:429 已接入，含 save/load/list/delete/rename
- [ ] **B2 /session save** — 保存当前对话
- [ ] **B3 /session list** — 列出已保存会话
- [ ] **B4 更新 README.md** — 反映实际完成状态

## 验证标准
- DAG: Pipeline 注册成功 → 执行后节点状态正确
- Session: 保存/加载/列出会话正常工作
- 代码编译无错误
# 删除整个 DAG（任务编排）系统

## 目标
从项目中完全移除 DAG 任务编排系统相关的所有代码，包括核心 dag 模块、dag_tools 工具集、及其在入口文件中的引用。

## 执行步骤

- [x] 步骤1：删除 `src/dag/` 整个目录（核心 DAG 模块）
- [x] 步骤2：删除 `src/tools/dag_tools/` 整个目录（DAG 工具集）
- [x] 步骤3：编辑 `src/lib.rs`，移除 `pub mod dag;`
- [x] 步骤4：编辑 `src/tools/mod.rs`，移除 `pub mod dag_tools;`
- [x] 步骤5：编辑 `src/agent.rs`，移除 DAG 上下文初始化和工具注册代码
- [x] 步骤6：删除 DAG 相关文档
- [x] 步骤7：运行 `cargo check` 验证编译通过

## 验证标准
- `cargo check` 通过，无报错
- 所有 DAG 相关引用已清除
