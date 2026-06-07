# 重要发现与关键决策

## 上下文压缩相关

### 1. auto_compress v2 修复（effective_max_turns）
- **问题**：原公式 `effective = max_turns * (trigger_threshold / current_tokens)` 在 tokens 刚过阈值时几乎不降低（0.996 的系数），导致滑动窗口无法触发
- **修复**：改用线性插值 `reduction_ratio = (current - threshold) / (limit - threshold)`，`effective = max_turns * (1 - reduction_ratio)`
- **效果**：70% tokens → 20轮, 78% → 15轮, 86% → 9轮, 94% → 4轮, 100% → 1轮
- **位置**：`src/context/strategy.rs` 第417-429行

### 2. hard_truncate 触发条件修复
- **问题**：使用 `>` 而非 `>=`，恰好 100% 时不会触发
- **修复**：`>=` 确保 100% 时也能触发
- **位置**：`src/context/strategy.rs` 第458行

### 3. 异步摘要注入机制
- **关键修复**：摘要注入时删除被摘要的原始消息，确保 token 真正下降
- **压缩前快照**：在 auto_compress 前保存消息快照，滑动窗口/硬截断后使用快照派发摘要任务，让摘要器能看到被删的消息
- **位置**：`src/context/mod.rs` 第137-156行（check_and_compress）

### 4. 集成测试 Tokio 运行时问题
- **问题**：`test_agent_loop_simulation_with_compression` 使用 `#[test]` 但调用 `tokio::spawn`（通过 `setup_summary_channel`）
- **修复**：在测试中创建 `tokio::runtime::Runtime` 并 `enter()` 上下文
- **位置**：`src/context/mod.rs` 第1097-1098行

## spawn_agent 工具
- 已实现：`src/tools/subagent/mod.rs` — spawn_agent 工具
- 已注册：`src/tools/mod.rs`
- CLI 支持：`--task` 参数，单次运行模式
- 待处理：更新系统提示词告知 agent 该工具的存在

## 测试覆盖
- 总计 82 个测试全部通过
- 核心压缩测试：6 个增量测试 + 1 个集成测试
- 测试类型覆盖：Token 缓存一致性、动态 max_turns、摘要注入、端到端生命周期、渐进压缩顺序、真实 Agent 循环模拟

### 5. 清理死代码 `maybe_dispatch_summary`
- **背景**：异步摘要派发逻辑已从 `check_and_compress` 集成到 `auto_compress` 内部（作为层1），旧的 `maybe_dispatch_summary` 方法成为死代码
- **操作**：删除 `src/context/mod.rs` 中的 `maybe_dispatch_summary` 方法（第181-201行）
- **原因**：`auto_compress` 已在层1位置（工具修剪之后、滑动窗口之前）派发异步摘要，不再需要外部调用

### 6. 修复策略.rs 测试编译错误
- **问题**：`auto_compress` 函数签名增加了第5个参数 `summary_tx`，但 `strategy.rs` 中的5个单元测试仍用4个参数调用
- **修复**：补充 `None` 作为第5个参数到所有5个测试调用点
- **受影响测试**：`test_auto_with_tool_pruning_first`、`test_auto_sliding_window_first`、`test_auto_hard_truncate`、`test_auto_not_needed`、`test_progressive_compression_layers`
- **结果**：`cargo check` 通过，82个测试全部通过

### 5. 🚨 紧急截断（层4）— 压缩失败时的最后安全网
- **问题**：所有 4 层压缩（工具修剪→异步摘要→滑动窗口→保底截断）都执行后，如果 Token 仍然超限（例如所有非 System 消息都是 protected），没有任何阻止机制，直接返回 `NotNeeded`
- **修复**：新增 `emergency_truncate()` 函数作为层4，当 hard_truncate 无法截断但 Token 仍然超限时调用：
  - 仅保留 System 消息 + 最后 2 轮对话
  - 忽略 preserved 标记（System 消息除外）
  - 如果仍然超限，继续删除最早的非 System 消息直到低于 limit
- **新增枚举**：`CompressResult::EmergencyTruncated { removed_count, kept_count }`
- **位置**：
  - `src/context/types.rs` — 新增 EmergencyTruncated 变体
  - `src/context/strategy.rs` — 新增 emergency_truncate() 函数（约310行），修改 auto_compress() 在 hard_truncate 失败后调用
  - `src/context/mod.rs` — handle_compress_result() 中处理 EmergencyTruncated
- **验证**：cargo check 通过，cargo test 82 passed

### 7. 启用 LLM 异步摘要（clone_box 模式）
- **背景**：`main.rs:167` 调用 `ctx.setup_summary_channel(None)`，异步摘要器因为没有 ModelAdapter 而始终退化为规则摘要
- **根因**：`ModelAdapter` trait 没有提供克隆能力，`query_client`（Box\<dyn ModelAdapter\>）无法复制一份给摘要器
- **修复**：
  - 在 `ModelAdapter` trait 中添加 `clone_box(&self) -> Box<dyn ModelAdapter>` 方法
  - 实现 `Clone for Box<dyn ModelAdapter>` 委托给 `clone_box`
  - `OpenAiCompatibleAdapter` 实现 `clone_box`（调用 `self.clone()`，`#[derive(Clone)]` 已有）
  - `main.rs:167` 改为 `ctx.setup_summary_channel(Some(query_client.clone()))`
- **效果**：异步摘要器现在能调用 LLM 生成结构化摘要（格式：目标 → 操作 → 决策 → 状态），LLM 不可用时自动降级为规则摘要
- **位置**：
  - `src/model/types.rs` — `clone_box` trait 方法 + `Clone for Box<dyn ModelAdapter>`
  - `src/model/openai_compatible.rs` — `clone_box` 实现
  - `src/main.rs:167` — 传入 `Some(query_client.clone())`

### 8. 修复 sanitize_name 测试失败
- **问题**：`sanitize_name("test/session")` 预期返回 `"testsession"`（移除斜杠），但函数将所有非字母/数字/连字符/下划线/点号字符都替换为 `_`，导致返回 `"test_session"`
- **根因**：`sanitize_name` 函数未区分空格（应替换为 `_` 以保持可读性）和其他特殊字符（应移除）
- **修复**：空格→下划线，其他特殊字符→空格→过滤移除
- **验证**：cargo test 85 passed（原来84 passed + 这个修复）

### 9. 新增 `/` 命令系统（命令发现 + 帮助） ✅
- **背景**：CLI 交互模式缺少统一的命令发现和管理系统，斜杠命令（/help, /clear, /session 等）分散在 main.rs 主循环中，没有集中管理
- **方案**：创建 `src/commands/mod.rs`，包含：
  - `Command` 结构体（名称、描述、用法、示例、子命令）
  - `CommandRegistry` 注册表（注册、查找、排序）
  - 内置命令注册：help, clear, session, sessions
  - 格式化输出：简短列表（print_help_short）、完整帮助（print_help_full）、单命令帮助（print_command_help）
  - 未知命令提示（print_unknown_command）
  - 集成到 main.rs 主循环（`/` 路由）
  - 5 个单元测试覆盖注册、查找、排序、格式化
- **状态**：✅ 全部完成（cargo check 通过, cargo test 90 passed）
