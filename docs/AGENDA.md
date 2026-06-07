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
