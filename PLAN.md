# 模型切换与整合重构计划

## 目标
将 Agent 从单模型适配器模式完全迁移到 ModelManager 多模型管理模式，并实现运行时模型切换的 `/model` 命令。

## 步骤

1. **修改 `main.rs`**：使用 `ModelManager::from_env()` 替代手动创建单适配器
2. **改造 `AgentBuilder`**：`model` 字段改为 `model_manager: Option<ModelManager>`，移除 `model()` 方法，改为 `model_manager()` 方法
3. **移除 `Agent::new()`**：不再需要向后兼容的便捷构造函数
4. **移除 `ModelManager::from_adapter()`**：不再需要向后兼容包装
5. **添加 `/model` 命令注册**：在 CommandRegistry 中注册 model 命令（list/switch 子命令）
6. **实现 `/model` 命令处理**：在 agent.rs 的主循环中添加 `/model list` 和 `/model switch <name>` 处理
7. **验证编译通过**
