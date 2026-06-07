

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
