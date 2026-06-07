

# P0: 解决上下文窗口满了不能发送消息

## 目标
1. 添加阻塞检测机制 — Token 超过 95% 时阻止发送消息
2. 添加强制压缩机制 — 阻塞时自动触发更激进的压缩
3. 修复 auto-loop 模式下滑动窗口无法压缩的问题
4. 启用 LLM 摘要 — 传入 ModelAdapter，让异步摘要使用真实 LLM

## 执行步骤

### ✅ 已完成
- [x] **步骤1**: `context/types.rs` — 添加 `CompressResult::ForceCompressed` 变体

### ⬜ 待执行

- [x] **步骤2**: `context/strategy.rs` — 添加 `force_compress` 函数
  - 添加了独立的 `pub fn force_compress()` 函数，跳过 trigger_threshold 检查直接执行最激进压缩
  - 五层强制压缩：工具修剪 → 异步摘要 → 滑动窗口到1轮（auto-loop 模式按消息数压缩）→ 硬截断 → 紧急截断
  - 修复 auto-loop 模式下 `count_turns` 只有 1 轮时滑动窗口跳过的 Bug：当 `turns <= 1` 时按消息数量压缩

- [x] **步骤3**: `context/mod.rs` — 添加 `is_blocked()`, `is_critical()`, `force_compress()` 方法
  - `is_blocked()`: `usage_ratio >= 0.95` ✓
  - `is_critical()`: `usage_ratio >= 0.90` ✓
  - `force_compress()`: 调用 `force_compress_strategy(...)` ✓

- [x] **步骤4**: `main.rs` — 在调用模型前添加阻塞检查
  - 调用 `stream_chat` 前检查 `ctx.is_blocked()`，阻塞时先 `ctx.force_compress()` 再发送 ✓
  - `ctx.is_critical()` 时执行轻量工具修剪 ✓

- [x] **步骤5**: `cargo check` 验证编译 + `cargo test` 验证通过
  - `cargo check` ✅ 编译成功
  - `cargo test` ✅ 90 tests passed
  - `spawn_agent` 自我验证 ✅

### 验证标准
- `cargo check` 通过
- `cargo test` 全部通过
- 模拟场景：长时间 auto-loop 后 token 稳定在 90% 以下，不会达到 100%
- spawn_agent 自我验证通过
