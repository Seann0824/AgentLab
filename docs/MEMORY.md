# MEMORY.md — 重要记录

## 2024-06-07: 修复上下文自动压缩不生效

### 问题描述
上下文使用率达到 100% 时，压缩机制没有被触发。

### 根因分析
`auto_compress` 中的滑动窗口（层1）触发条件仅依赖轮数（`turns > max_turns`，默认 20 轮）。

当 token 使用率在 `trigger_threshold`（70%）到 `token_limit`（100%）之间时：
1. **层0（工具修剪）**：如果工具结果本身不长（<= 200 chars）或没有工具结果 → 不生效
2. **层1（滑动窗口）**：如果轮数 <= 20 → 不触发
3. **层3（保底截断）**：如果 token <= 128000 → 不触发

最终返回 `NotNeeded`，没有任何压缩发生。

### 修复方案
在 `auto_compress` 的情况 C 中，增加基于 token 数的动态 `effective_max_turns` 计算：

- 当 `current_tokens >= trigger_threshold` 时，按超出比例降低有效 max_turns
- 公式：`effective_max_turns = max_turns * (trigger_threshold / current_tokens)`
- 举例：token 100%（128000）→ effective = 20 * 0.7 = 14，15轮时触发压缩
- 安全边界：`max(1, min(effective, max_turns))`

### 修改文件
- `src/context/strategy.rs`：`auto_compress` 函数中的情况 C（第396-432行）

### 验证
- `cargo check` 通过
- `cargo test` 全部 64 个测试通过


## 2024-06-07: 结构化任务执行框架（TaskManager）

### 问题描述
Agent 在扁平循环中缺乏任务意识——上下文压缩后丢失进度，没有自动的任务状态持久化机制。

### 设计决策
引入语言无关的 TaskManager，核心设计原则：
1. **代码+文件双重状态管理** — TaskManager 维护内存中的结构化状态，同时读写 PLAN.md / AGENDA.md / MEMORY.md 作为持久化
2. **压缩感知注入** — 上下文压缩后自动注入当前任务状态，帮助模型恢复上下文
3. **去重保护** — 状态未变化时不重复注入，避免上下文膨胀
4. **Agent 可编辑** — 状态文件是 Markdown 格式，Agent 可通过 edit 工具直接修改，TaskManager 也支持通过 load() 读取

### 修改文件
| 文件 | 操作 |
|------|------|
| `src/task/mod.rs` | 新增 — TaskManager 结构体 + 文件读写 + 状态注入 |
| `src/task/types.rs` | 新增 — TaskState 数据类型 + 序列化 |
| `src/main.rs` | 修改 — 注册 task 模块，主循环中集成 TaskManager |
| `Cargo.toml` | 修改 — 添加 tempfile 作为 dev-dependency |

### 验证
- `cargo check` 通过
- `cargo test` 75 passed（11 个新测试 + 64 个原有测试）
