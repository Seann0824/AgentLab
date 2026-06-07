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
