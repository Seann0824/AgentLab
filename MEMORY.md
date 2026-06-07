

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
