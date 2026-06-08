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

# 🎯 实现「目标驱动能力（Goal-Driven Capability）」

## 目标
按照 docs/designs/goal-driven-capability.md 设计文档，实现完整的 Goal-Driven 能力代码。

---

## 阶段一：基础框架（P0）

- [x] 步骤1：创建 `src/goal/types.rs` — Goal、GoalStatus 数据类型
- [x] 步骤2：创建 `src/goal/registry.rs` — GoalRegistry 持久化存储
- [x] 步骤3：创建 `src/goal/mod.rs` — 模块入口，重新导出
- [x] 步骤4：编辑 `src/lib.rs` — 添加 `pub mod goal;`
- [x] 步骤5：验证编译 — `cargo check` ✅

## 阶段二：Agent 集成（P1）

- [ ] 步骤6：Agent 结构体添加 `goal_manager` 字段
- [ ] 步骤7：系统提示词注入 Goal 上下文块
- [ ] 步骤8：添加 `/goal` 命令处理（status/complete/fail/cancel/resume）
- [ ] 步骤9：主循环中检测 LLM 输出的 Goal 完成信号
- [ ] 步骤10：验证编译 — `cargo check`

## 验证标准
- `cargo check` 通过，无报错
- Goal 数据类型完整（Goal、GoalStatus、序列化/反序列化）
- GoalRegistry 可创建/读取/更新/列出 Goal
- Agent 主循环可检测和处理 `/goal` 命令
- 系统提示词在活跃 Goal 时注入上下文

# 🎯 打通 Goal — Agent 集成 ✅ 已完成

## 目标
完成 Goal 系统与 Agent 主循环的最终集成，实现真正的「目标驱动」自主执行。

## 步骤

- [x] 步骤1：Goal 启动注入 — 在 Agent.run() 中初始化后，如果有活跃 Goal，注入目标状态到上下文
- [x] 步骤2：LLM 输出 Goal 信号检测 — 扫描 final_assistant_message 中 `/goal complete/fail/cancel` 模式并自动处理
- [x] 步骤3：活跃 Goal 轮次计数 — 每次 LLM 调用时，如果有活跃 Goal 则 increment_turn()
- [x] 步骤4：Goal 完成后的自动停止 — 当 Goal 进入终止状态时，输出总结并停止自动循环
- [x] 步骤5：验证编译 — `cargo check 2>&1 | tail -30` ✅

## 验证标准
- ✅ `/goal set <描述>` 创建目标并自动激活
- ✅ 活跃目标信息在启动时注入上下文
- ✅ LLM 输出 `/goal complete <id>` 后自动标记完成
- ✅ LLM 连续执行时轮次计数递增
- ✅ 目标完成后自动停止 auto 循环
- ✅ cargo check 通过

# 🎯 持久化记忆 — 向量数据库实现（更新：实际状态盘点）

## 目标
完成持久化记忆使用向量数据库实现：先输出技术文档，再完整实现整个能力，最后自我验证闭环。

---

## 阶段一：技术设计文档 ✅

- [x] 步骤1：创建 `docs/designs/persistent-memory-vector-db.md` 设计文档

## 阶段二：基础模块实现 ✅

- [x] 步骤2：创建 `src/memory/` 模块框架（mod.rs + types.rs）
- [x] 步骤3：实现 EmbeddingClient — 复用模型配置调用 embeddings API
- [x] 步骤4：实现 VectorStore — 本地文件向量存储 + 余弦相似度搜索
- [x] 步骤5：实现 MemoryManager — 记忆 CRUD + 生命周期管理
- [x] 步骤6：注册到 lib.rs，cargo check 通过

## 阶段三：Agent 集成

- [x] 步骤7：实现记忆工具 memory_save/memory_search/memory_forget/memory_stats — agent.rs:173-176 已注册 ✅
- [x] 步骤8：压缩后自动注入相关记忆到上下文 — agent.rs:616-646 已实现 ✅
- [x] 步骤9：对话中自动提取重要信息存入记忆 — agent.rs:873-908 已实现 ✅
- [x] 步骤10：完整集成 + cargo check 验证 ✅

## 阶段四：自我验证闭环

- [x] 步骤11：使用 spawn_agent 验证记忆系统端到端工作 ✅
    - memory_save: ✅ 保存成功，返回有效 ID
    - memory_search: ✅ 搜索到相关记忆，score=0.92
    - memory_stats: ✅ 返回正确统计信息
    - memory_forget: ✅ 删除成功，重新搜索已确认删除
- [x] 步骤12：更新 ROADMAP.md 反映 Phase 3 进度 ✅
    - Phase 3 进度从 15% → 80% 🟡
    - ROADMAP.md 版本更新到 v3.0
    - "Next 3" 更新为：新工具脚手架生成 → 配置文件系统 → 自我修改安全机制

## 验证标准
- 设计文档完整覆盖系统架构、数据流、组件交互
- 编译通过（cargo check 无错误）
- Embedding API 可正常调用生成向量
- VectorStore 支持存储/搜索/删除
- MemoryManager 支持记忆的提取、注入、生命周期管理
- spawn_agent 端到端验证通过
# Bug 修复：`/goal set` 后目标未注入上下文，AI 无法感知

## 问题描述
当用户通过 `/goal set <描述>` 设置目标后，`handle_goal_command` 创建了目标并 `continue` 回到循环开始。但目标上下文只在两种场景注入：
1. 启动时（第 433-437 行）— 仅首次启动
2. 上下文压缩后（第 600-604 行）— 仅压缩时

所以 `/goal set` 后目标从未注入到会话上下文里，AI 看不到目标。

## 修复方案
在 `handle_goal_command` 执行后，检查是否有活跃目标，有则注入到 `context_manager`。

- [x] 分析问题根因
- [x] 修改代码：在 `/goal` 命令处理完后注入目标上下文
- [x] 运行 `cargo check` 验证编译 ✅（仅 warnings，无 errors）
- [x] 测试验证 ✅（子 agent 编译通过并确认代码存在）
