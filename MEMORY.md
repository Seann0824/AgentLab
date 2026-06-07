

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
