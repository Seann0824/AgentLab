

## [2025-01-xx] InvestigateTool 集成：自动错误快照 + 工具注册

### 关键决策
1. **注册 InvestigateTool**: 在 `default_tool_manager()` 中添加了 `InvestigateTool`，使其成为默认可用工具。
2. **自动捕获错误快照**: 在 Agent 主循环中，每次工具执行完成后，自动检测 `ok: false` 的工具结果，并调用 `ErrorSnapshotManager::capture()` 保存快照。
3. **快照输出**: 错误快照保存时，在 stderr 输出 `📸 错误快照已保存: <id> -> <path>`。
4. **[SNAPSHOT] 引用**: 在 `--task` 模式（子 agent）退出时，输出 `[SNAPSHOT] <id>` 引用最后的快照，方便主 agent 读取。

### 实现细节
- 快照捕获放在 `tool_results` 收集完成后、`tool_calls` 被移入 ChatMessage 之前
- 自动捕获只针对 `ok == false` 的工具结果
- 捕获内容包括：当前上下文消息、工具名称、工具参数、错误消息

## [2025-06-13] ModelManager 集成：self.model 迁移到 self.model_manager

### 关键决策
1. **Agent 结构体用 ModelManager 替代 Box<dyn ModelAdapter>**: Agent 现在持有 `ModelManager`，而不是直接的模型适配器。
2. **AgentBuilder::build() 和 Agent::new()** 使用 `ModelManager::from_adapter(model)` 将单个预构建的适配器包装到 ModelManager 中。
3. **ModelManager::from_adapter()**: 新增的构造方法，用于向后兼容旧的 `Agent::new(model)` 调用方式。

### 技术细节
- 修改的文件: `src/agent.rs`, `src/model/manager.rs`
- `agent.rs` 中所有 `self.model` 引用已替换为 `self.model_manager.current_adapter()` 调用
- `ModelManager` 提供了丰富的查询方法：`current_adapter()`, `list_models()`, `switch()`, `add_model()`, `clone_active_adapter()`
- 编译验证通过（仅 warnings，无 errors）

## [2025-06-13] DAG Pipeline 可观测性全面改进

### 问题
DAG Pipeline 执行过程是个黑盒：执行后只返回计数摘要（成功/失败/运行数），无法观测每个节点的具体输出和审核结果。

### 改动文件
1. **`src/tools/dag_tools/execute.rs`** — pipeline_execute 工具
   - 执行过程中输出实时进度到 stderr（🚀 开始、▶️ 节点执行、⏳ Worker/Reviewer 执行中、✅ 完成/❌ 失败）
   - 返回结果新增 `nodes` 字段，包含每个节点的完整信息：`worker_output`、`review_result`、`final_output`、`status`、`retry_count`、`logs`、`started_at`、`completed_at`

2. **`src/dag/runtime.rs`** — NodeRuntime 执行器
   - `execute_node()` 返回结构从 `{ "content": output }` 扩展为 `{ "content", "worker_output", "review": { "passed", "score", "feedback", "details" } }`
   - 修复了 `FailedAfterRetries` 模式匹配中 `last_worker_output` 未使用的警告

3. **`src/dag/engine.rs`** — DAGEngine 引擎
   - `on_node_completed()` 新增提取 `worker_output` 和 `review_result` 的逻辑，存储到 `NodeInstance`

4. **`src/tools/dag_tools/status.rs`** — pipeline_status 工具
   - 新增返回完整 `nodes` 细节（与 execute 返回的结构一致）
   - 可从引擎存储中读取已执行完成的 Pipeline 节点详细信息

### 效果
- **执行中**：stderr 实时输出每个节点的进度（开始→执行中→完成/失败）
- **执行后**：pipeline_execute 返回每个节点的 `worker_output`（Worker 完整输出）、`review_result`（审查评分/反馈/分项检查）、`final_output`（最终输出）
- **事后查询**：pipeline_status 可查看已执行 Pipeline 的完整节点细节

## [2025-06-08] 🎯 目标驱动能力设计文档

### 关键决策
1. **Goal 作为独立模块**：新增 `src/goal/` 模块，与现有 TaskManager 互补，不替代。
2. **JSON 持久化**：Goal 数据存入 `docs/goals/` 目录（index.json + goal_{id}.json），便于程序读写。
3. **LLM 驱动的自评估**：完成标准由 LLM 理解判定，代码只提供框架和 Prompt 注入，不做硬编码校验。
4. **与现有系统提示词松耦合**：Goal 上下文作为可选注入块（仅在 Active Goal 时注入），不影响普通模式。
5. **`/goal` 命令风格**：使用 `/goal complete`、`/goal fail`、`/goal cancel` 等命令，与现有 `/session`、`/debug` 一致。
6. **四个实现阶段**：P0 基础框架 → P1 Agent 集成 → P2 测试完善 → P3 进阶功能，可增量交付。

### 实现要点
- Goal 生命周期：Proposed → Active → Completed / Failed / Cancelled
- 防无限循环：最大轮次限制（100）、停滞检测（连续 N 轮无进展）、用户中断
- 自评估时机：步骤完成后、达到检查点、全部步骤完成后、遇到严重错误后
- 三种执行模式：对话模式（普通聊天）、目标驱动模式（自主执行）、子任务模式（--task）

## [2025-06-13] 🗺️ 路线图全面更新（v2.0）

### 关键决策
1. **新建 `docs/ROADMAP.md`**: 替代旧的 `docs/designs/agent-capability-roadmap.md`，作为唯一的路由图入口
2. **5 个 Phase 划分**: Phase 0(基础设施) → Phase 1(结构化执行) → Phase 2(自我进化) → Phase 3(持久记忆) → Phase 4(产品化)
3. **完成度量化**: 每个 Phase 用百分比 + 进度条清晰展示，每个能力用 ✅/🟡/🔴 标注
4. **优先级三维度**: P0(本月) / P1(下季度) / P2(未来)，聚焦 Next 3 行动项

### 现状总结
- 项目已完成核心基础设施（100%），处于从「工具型 Agent」向「自主 Agent」进化的关键节点
- 下一步核心：完成 Goal → Agent 集成（打通结构化执行的最后一环）
- 自我进化能力（Phase 2）是项目的核心竞争力所在，是区别于其他框架的关键差异点

## [2025-06-13] 🎯 Goal — Agent 集成全部完成

### 关键决策
1. **Goal 启动注入**: Agent.run() 初始化后，检测活跃 Goal 并通过 `get_inject_message()` 注入目标状态到上下文（agent.rs:408-414）
2. **压缩后再注入**: 上下文压缩后自动重新注入活跃目标状态（agent.rs:574-580）
3. **LLM 输出信号检测**: `extract_goal_signal()` 使用正则解析 LLM 回复中的 `/goal complete|fail|cancel <id>` 模式并自动处理（agent.rs:701-720）
4. **轮次计数与防无限循环**: 每次 LLM 调用后递增 turn_count，超限（100轮）或停滞（连续5轮无进展）自动标记为失败（agent.rs:682-699）
5. **目标完成自动停止**: 目标进入终止状态（Completed/Failed/Cancelled）后自动设置 `is_auto = false` 停止自动循环（agent.rs:722-728）

### 实现要点
- `/goal` 命令完整：set/list/status/complete/fail/cancel/history
- 系统提示词中已包含目标驱动模式的完整说明
- `cargo check` 通过（仅 warnings，无 errors）

### Phase 1 状态更新
- Goal → Agent 集成 ✅ → Phase 1 完成进度升至 **100%**


# 2025-01-27: MemoryManager 集成修复

## 问题
agent.rs 在 build() 方法中集成 MemoryManager 时有 3 个编译错误：
1. `MemoryManager::new(&self.current_dir)` 是异步的，而 `build()` 是同步方法
2. Memory tools 没有 `new()` 方法，需要使用 struct 字面量初始化
3. `memory_manager` 字段类型不匹配（需要 `Arc<Mutex<MemoryManager>>`）

## 修复
1. build() 中使用 `MemoryManager::new_mock()` 作为默认值（同步构造）
2. 用 `Arc::new(Mutex::new(memory_manager))` 包装
3. Memory tools 用结构体字面量 `MemorySaveTool { memory_manager: ... }` 初始化
4. Agent 结构体中 `memory_manager` 字段类型改为 `Arc<Mutex<MemoryManager>>`
5. `run()` 中的记忆检索调用改为 `self.memory_manager.lock().await.search_similar().await`
6. SearchResult 的 content 字段访问改为 `mem.record.content`
7. 添加 `async-stream = "0.3.6"` 依赖到 Cargo.toml

## 运行结果
- `cargo check` 通过（只有 warnings，无 errors）
---

# 2025-06-18: Phase 2 Step 1 ✅ — generate_tool 完成

## 完成内容
🎉 **新工具脚手架生成**（Phase 2 Step 1）已全部完成并端到端验证通过！

### 实现细节
- **工具名**: `generate_tool`
- **功能**: 根据工具名 + 参数描述 + 功能描述，自动生成完整的 Rust 工具脚手架代码
- **自动注册**: 自动在 `src/tools/mod.rs` 中添加 `pub mod <name>;`
- **注册路径**: `src/tools/generate_tool/` → `src/tools/mod.rs` → `src/agent.rs`

### 验证结果
- `cargo check` 编译通过 ✅
- spawn_agent 端到端验证：生成 hello_world 工具 → 注册 → cargo check ✅

### 关键决策
1. **代码生成采用 `format!` 模板 + 字符串拼接**：不引入额外模板引擎依赖
2. **JSON Schema 参数映射**：string → string, number → f64, boolean → bool, array → Vec<String>, object → serde_json::Value
3. **注册分两步**：自动注册到 `src/tools/mod.rs`，需手动在 `agent.rs` 注册（提示用户完成）
4. **不自动注册到 agent.rs**: 避免执行时上下文不一致

## 下一步
**Phase 4 Step 1 — 配置文件系统 [P0]**
开始实现 YAML/TOML 配置文件系统。
