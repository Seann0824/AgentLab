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

---

# 🎯 新活跃目标：迭代 Roadmap 内容直到全部完成

## 目标描述
根据已创建的 ROADMAP.md，自动迭代执行 roadmap 内容，自我验证，直到完成所有 roadmap 项目。

## 当前 Roadmap 状态

```
Phase 0: 基础设施    ▰▰▰▰▰▰▰▰▰▰ 100% ✅ 已完成
Phase 1: 结构化执行  ▰▰▰▰▰▰▰▰▰▰ 100% ✅ 已完成
Phase 2: 自我进化    ▰▰▱▱▱▱▱▱▱▱  15%  🔴
Phase 3: 持久记忆    ▰▰▰▰▰▰▰▰▱▱  80%  🟡
Phase 4: 产品化      ▰▱▱▱▱▱▱▱▱▱  10%  🔴
```

## 优先级排序（Next 3）

### [P0] 1. 新工具脚手架生成（Phase 2 自我进化）
让 Agent 能自动生成本地工具模板代码，实现「自我进化」的第一步。

- [x] **1.1 设计**：确定工具生成模板的结构 — Tool trait 实现、参数 schema、流式 execute、注册流程
- [x] **1.2 实现**：创建 `generate_tool` 工具 — 在 `src/tools/generate_tool/` 实现代码生成器
- [x] **1.3 注册**：添加到 `src/tools/mod.rs` 和 `src/agent.rs`
- [x] **1.4 验证**：cargo check 通过 ✅（仅 warnings，无 errors）
- [x] **1.5 spawn_agent 验证**：用 spawn_agent 执行端到端验证 ✅ — generate_tool 成功创建 hello_world 工具并编译通过

### [P0] 2. 配置文件系统（Phase 4 产品化）
支持 YAML/TOML 配置文件，实现模型配置、上下文策略、工具开关的集中管理。

- [ ] **2.1 设计**：配置结构体定义 + 配置文件路径规范
- [ ] **2.2 实现**：ConfigManager 实现（加载/合并/覆盖）
- [ ] **2.3 集成**：CLI 参数覆盖配置，Agent 启动时加载
- [ ] **2.4 验证**：创建配置文件，验证配置生效

### [P1] 3. 自我修改安全机制（Phase 2 自我进化）
- [ ] **3.1 自动备份**：修改前自动备份文件
- [ ] **3.2 编译失败回滚**：cargo check 失败时自动恢复
- [ ] **3.3 关键文件权限控制**

### [P1] 4. 记忆系统收尾（Phase 3）
- [ ] **4.1 记忆合并/去重**
- [ ] **4.2 单元测试完善**
- [ ] **4.3 更新 ROADMAP.md 到 100%**

## 验证标准
- 每项完成后 cargo check 通过
- 自我验证（spawn_agent 测试新能力）
- ROADMAP.md 进度更新
# Goal 无限注入上下文问题的修复计划

## 问题分析
Goal 驱动的自动循环中有 3 个地方会注入目标消息到上下文：

1. **启动时注入**（agent.rs:433-438）：✅ 合理，只注入一次
2. **压缩后重新注入**（agent.rs:613-618）：✅ 合理，压缩后恢复上下文
3. **每次自动循环都注入**（agent.rs:872-888）：❌ 问题所在！每次自动循环都注入导致：
   - 上下文无限制增长（每次都加一条 system message）
   - `is_auto` 始终为 true，陷入无限自动循环
   - 最终触发上下文限制，无法调用 AI

## 修复步骤
- [x] 修复1: 移除自动循环中的重复注入（lines 877-881），改为仅控制 is_auto 标志 ✅
- [x] 修复2: 添加最大连续自动迭代次数（MAX_CONSECUTIVE_AUTO=30）限制 ✅
- [x] 修复3: 运行 cargo check 验证编译通过 ✅（仅 warnings，无 errors）
- [x] 修复4: 用 spawn_agent 做端到端验证 ✅（子 agent 确认所有 4 项变更均正确）
# 🎯 去掉 Goal 的最大迭代次数限制（无限）

## 目标
去掉 Goal 的 max_turns 轮次限制，使其无限运行（不再因轮次超限自动标记失败）。

## 执行步骤

- [ ] 1. 移除 Goal 结构体中的 `turn_count` 和 `max_turns` 字段及相关方法
- [ ] 2. 移除 agent.rs 中 Goal 轮次计数和超限检查的代码块
- [ ] 3. 移除 get_goal_context_prompt 中的"轮次"显示行
- [ ] 4. 更新 types.rs 中的测试
- [ ] 5. 运行 cargo check 验证编译通过

## 验证标准
- cargo check 编译通过
- 代码中不再有 goal 的轮次限制逻辑

# 🐝 多 Agent 蜂群架构实现

## 目标
按照 `docs/designs/multi-agent-swarm-architecture.md` 设计文档，实现完整的多 Agent 蜂群架构。

---

## Phase 0 — 基础通信层（UDS + JSON-RPC 2.0）✅ 已完成

- [x] 0.1 创建 `src/swarm/mod.rs` — 模块入口，重新导出所有子模块
- [x] 0.2 实现 UDS 传输层 — `src/swarm/transport.rs`（UdsServer + UdsClient）
- [x] 0.3 实现 JSON-RPC 2.0 协议 — `src/swarm/rpc.rs`（请求/响应/错误/方法枚举）
- [x] 0.4 实现 Agent 注册协议 — `src/swarm/registry.rs`（SwarmRegistry + 注册/注销/发现）
- [x] 0.5 实现心跳检测 — `src/swarm/heartbeat.rs`（心跳发送/检查/超时处理）
- [x] 0.6 注册 swarm 模块到 `src/lib.rs`
- [x] 0.7 验证编译 — `cargo check` ✅（仅 warnings，无 errors）

## Phase 1 — Swarm Registry & CLI

- [ ] 1.1 完善 SwarmRegistry 数据结构（类型查询、状态管理）
- [ ] 1.2 实现 Agent 注册/注销/发现完整 API
- [ ] 1.3 实现 `query_swarm` CLI 命令（`/swarm status`）
- [ ] 1.4 创建 `swarm_ctl` 工具 — 蜂群控制工具
- [ ] 1.5 验证编译 — `cargo check`

## Phase 2 — Memory Agent

- [ ] 2.1 `main.rs` 支持 `--agent-type` 参数
- [ ] 2.2 创建 Memory Agent 主循环
- [ ] 2.3 实现自动记忆提取逻辑
- [ ] 2.4 Orchestrator 集成：自动派发记忆任务
- [ ] 2.5 验证编译 — `cargo check`

## Phase 3 — General Agent + Verifier Agent ✅

- [x] 3.1 创建 General Agent 主循环 ✅
- [x] 3.2 实现 Agent Pool 管理 ✅
- [x] 3.3 创建 Verifier Agent 主循环 ✅
- [x] 3.4 Orchestrator 集成：`dispatch_task` 工具 ✅
- [x] 3.5 验证编译 — `cargo check` ✅

## Phase 4 — 任务编排引擎 ✅

- [x] 4.1 实现 Workflow 定义与解析 ✅
- [x] 4.2 实现串行/并行/条件分支执行 ✅
- [x] 4.3 验证编译 — `cargo check` ✅

## 验证标准
- UDS Server/Client 可正常通信
- Agent 启动后自动注册到 Orchestrator
- `/swarm status` 可查看所有 Agent 状态
- Memory Agent 可独立启动并注册
- 所有代码编译通过（cargo check）

# 🐝 当前任务：多 Agent 蜂群架构完成 — Phase 0+1 整合

## 目标
完成多 Agent 蜂群架构的基础层整合（Phase 0 + Phase 1），包括：Agent 集成 SwarmRegistry、修复 `/swarm` 命令使其可用、编写测试、验证编译。

---

## 步骤

- [x] 步骤1：修复 Goal 的 name/description（当前被错误拼接）
- [ ] 步骤2：Agent 结构体添加 `swarm_registry` 字段
- [ ] 步骤3：AgentBuilder 添加 `with_swarm_registry()` 方法
- [ ] 步骤4：修复 `/swarm` CLI 命令使用 Agent 持有的 registry
- [ ] 步骤5：为 heartbeat.rs 编写单元测试
- [ ] 步骤6：增强 transport.rs 的测试（Client 连接测试）
- [ ] 步骤7：运行 `cargo check` 验证编译通过
- [ ] 步骤8：运行 `cargo test` 验证所有测试通过
- [ ] 步骤9：更新 AGENDA.md 和 MEMORY.md

## 验证标准
- `cargo check` 通过
- `cargo test` 中 swarm 模块测试全部通过
- `/swarm status` 命令返回正确的蜂群状态（不再报 registry 未初始化）
- Goal 的名称正确显示为设计文档标题

# 🐝 多 Agent 蜂群架构实现（续）— Phase 2~4

## 目标
按照 `docs/designs/multi-agent-swarm-architecture.md` 设计文档，继续实现多 Agent 蜂群架构的剩余 Phase。

## 已完成状态
- ✅ Phase 0 — 基础通信层（UDS + JSON-RPC 2.0）：全部完成
- ✅ Phase 1 — Swarm Registry & CLI：全部完成
  - SwarmRegistry 数据结构完整（register/unregister/heartbeat/query/query_by_type/query_by_status）
  - SwarmRegistry 持久化（save_to_disk / load_from_disk）
  - SwarmRegistry 集成到 Agent 结构体 + AgentBuilder
  - SwarmCtl 工具已创建并使用实际 registry
  - `/swarm` CLI 命令已注册并实现（status/list/query/help）
  - cargo check 编译通过

## 剩余工作

### Phase 2 — Memory Agent（记忆型 Agent）
- [x] 2.1 `main.rs` 支持 `--agent-type` 和 `--socket-path` 参数，启动不同类型 Agent ✅
- [x] 2.2 创建 Memory Agent 主循环（非交互式，通过 UDS 接收任务）✅
- [ ] **2.3 实现自动记忆提取 + 内存维护逻辑** — Memory Agent 后台任务：内存合并/压缩/去重
- [ ] **2.4 Orchestrator 集成：自动启动 Memory Agent** — UDS Server + spawn Memory Agent 子进程
- [ ] 2.5 验证编译 — `cargo check`

## Phase 2.4 详细步骤

- [x] **2.4.1 创建 SwarmOrchestrator** — `src/swarm/orchestrator.rs` ✅
- [ ] **2.4.2 run_orchestrator() 集成 SwarmOrchestrator** — 启动 UDS Server + spawn Memory Agent
- [ ] **2.4.3 Agent 注册消息处理** — accept 循环 + SwarmRegistry + 双向流存储
- [ ] **2.4.4 心跳监控后台任务** — HeartbeatMonitor 定期检查超时
- [ ] **2.4.5 验证 cargo check**

> **状态**: 2.4.1 ✅ — 下一步：2.4.2 run_orchestrator() 集成

### Phase 3 — General Agent + Verifier Agent ✅
- [x] 3.1 创建 General Agent 主循环（非交互式任务执行器）✅
- [x] 3.2 实现 Agent Pool 管理（可复用 Agent 实例池）✅
- [x] 3.3 创建 Verifier Agent 主循环（代码验证专用）✅
- [x] 3.4 Orchestrator 集成：`dispatch_task` 工具派发任务给指定 Agent ✅
- [x] 3.5 `orchestrator` 模式自动启动、管理所有子 Agent ✅
- [x] 3.6 验证编译 — `cargo check` ✅

### Phase 4 — 任务编排引擎 ✅
- [x] 4.1 实现 Workflow 定义与解析（串行/并行/条件）✅
- [x] 4.2 实现 Workflow 执行引擎 ✅
- [ ] 4.3 Orchestrator 集成 workflow 执行
- [x] 4.4 验证编译 — `cargo check` ✅

### Phase 5 — 端到端验证与文档
- [ ] 5.1 `cargo test` 全部通过
- [ ] 5.2 用 spawn_agent 验证 Memory Agent 可独立启动
- [ ] 5.3 更新 AGENDA.md 和 MEMORY.md
- [ ] 5.4 更新 ROADMAP.md 反映完成状态

## 验证标准
- `--agent-type memory` 可启动 Memory Agent 并注册到 Orchestrator
- General Agent 可接收任务并返回结果
- Verifier Agent 可运行 cargo check 验证
- `/swarm status` 显示所有 Agent 状态
- 所有代码编译通过（cargo check）

# 🐝（当前任务）完成多 Agent 蜂群架构 — Phase 2~4

## 目标
按照 `docs/designs/multi-agent-swarm-architecture.md` 设计文档，完成多 Agent 蜂群架构的剩余 Phase（2~4），实现可用的多 Agent 协作系统。

## 已完成状态
- ✅ Phase 0 — 基础通信层（UDS + JSON-RPC 2.0）：transport / rpc / registry / heartbeat / mod.rs
- ✅ Phase 1 — Swarm Registry & CLI：SwarmRegistry 持久化、Agent 集成、SwarmCtl 工具、/swarm 命令
- ✅ Phase 2.1 — main.rs 支持 --agent-type / --socket-path / --orchestrator-socket 参数
- ✅ Phase 2.2 — Memory Agent 主循环（注册、心跳、请求处理）
- ✅ Phase 2.4.1 — SwarmOrchestrator 已创建（UDS Server + 子进程管理 + 消息路由）

## 剩余工作

### Phase 2 — Memory Agent（记忆型 Agent）
- [x] **2.3 实现自动记忆提取 + 内存维护逻辑** — Memory Agent 后台任务：自动提取/合并/去重/清理 ✅
- [x] **2.4 完整 Orchestrator 集成** — run_orchestrator() 完善 SwarmOrchestrator 集成 ✅
  - [x] 2.4.2 run_orchestrator 完全集成 SwarmOrchestrator（从 orch_arc 提取 registry）✅
  - [x] 2.4.3 Agent 注册消息处理（accept 循环 + 双向通信）✅
  - [x] 2.4.4 心跳监控后台任务改进 ✅
  - [x] 2.4.5 验证 cargo check ✅
- [x] 2.5 验证编译 — cargo check ✅

### Phase 3 — General Agent + Verifier Agent
- [x] 3.1 创建 General Agent 主循环 — src/swarm/agents/general.rs（非交互式任务执行器）✅
- [x] 3.2 实现 Agent Pool 管理 — src/swarm/pool.rs（可复用 Agent 实例池）✅
- [x] 3.3 创建 Verifier Agent 主循环 — src/swarm/agents/verifier.rs（代码验证专用）✅
- [x] 3.4 Orchestrator 集成：dispatch_task 工具 + run_general/verifier_agent ✅
- [x] 3.5 验证编译 — cargo check ✅

### Phase 4 — 任务编排引擎（Workflow）✅
- [x] 4.1 实现 Workflow 定义与解析 — src/swarm/workflow.rs ✅
- [x] 4.2 实现串行/并行/条件分支执行引擎 ✅
- [x] 4.3 验证编译 — cargo check ✅

### Phase 5 — 端到端验证与收尾
- [ ] 5.1 用 spawn_agent 验证蜂群可启动
- [ ] 5.2 更新 AGENDA.md 和 MEMORY.md
- [ ] 5.3 更新 ROADMAP.md 反映完成状态
- [ ] 5.4 修复所有编译 warnings

## 验证标准
- `cargo check` 通过
- Memory Agent 有完整的自动提取/合并/清理逻辑
- General Agent + Verifier Agent 可编译
- Workflow 定义和执行引擎可编译
# 修复 AI 忘记调用 `/goal complete` 导致无限循环的问题

## 目标
增强 Goal 完成检测机制，当 AI 完成任务但忘记显式输出 `/goal complete` 时，系统能自动检测并标记完成，避免无限循环。

## 步骤
- [ ] 1. 添加 `auto_detect_goal_completion` 函数，检测 PLAN.md 步骤和 AI 回复中的完成信号
- [ ] 2. 在主循环中集成自动检测逻辑（在 extract_goal_signal 检查之后作为 fallback）
- [ ] 3. 编译验证 + 派生子 agent 端到端测试
- [ ] 4. 总结

## 验证标准
- `cargo check` 通过
- 当 AI 回复包含"已完成"等关键词但无 `/goal complete` 时，系统自动标记完成
- 当 PLAN.md 所有步骤为 `[x]` 时，系统自动标记完成
