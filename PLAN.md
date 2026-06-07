# 结构化任务执行框架（TaskManager）

## 目标
在 Agent 中引入结构化任务执行框架，让 Agent 能在多轮对话中保持任务状态，在上下文压缩后能自动恢复进度。

核心能力（语言无关）：
- 任务状态持久化（PLAN.md / AGENDA.md / MEMORY.md 由代码自动维护）
- 上下文压缩后自动注入当前任务状态
- 支持任务开始、步骤完成、重要发现等生命周期

## 执行步骤

- [x] 1. 创建 `src/task/mod.rs` — TaskManager 核心结构体 + 文件读写
- [x] 2. 创建 `src/task/types.rs` — 任务状态数据类型
- [x] 3. 在 `main.rs` 中注册 `task` 模块
- [x] 4. 修改 `main.rs` 主循环，在上下文压缩后注入任务状态
- [x] 5. 运行 `cargo check` 验证编译通过
- [x] 6. 运行 `cargo test` 确保不影响现有功能（75 passed）

## 验证标准
- [x] `cargo check` 通过
- [x] 所有现有测试通过（75 passed）
- [x] TaskManager 能正确读写状态文件（test_save_and_load_roundtrip + test_load_from_files）
- [x] 压缩后能生成有效的状态提示注入上下文（test_get_inject_message 系列）
