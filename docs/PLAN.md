

# P0: 解决上下文窗口满了不能发送消息

## 目标
1. 添加阻塞检测机制 — Token 超过 95% 时阻止发送消息
2. 添加强制压缩机制 — 阻塞时自动触发更激进的压缩
3. 启用 LLM 摘要 — 传入 ModelAdapter，让异步摘要使用真实 LLM

## 执行步骤

- [ ] **步骤1**: `context/types.rs` — 添加 `CompressResult::ForceCompressed` 变体
- [ ] **步骤2**: `context/strategy.rs` — 修改 `auto_compress` 添加 `force` 参数，调整为更激进的压缩阈值
- [ ] **步骤3**: `context/mod.rs` — 添加 `is_blocked()`, `is_critical()`, `force_compress()` 方法
- [ ] **步骤4**: `main.rs` — 创建第二个模型客户端用于摘要器；添加阻塞检查逻辑
- [ ] **步骤5**: `cargo check` 验证编译

- [x] **步骤1**: `context/types.rs` — 添加 `CompressResult::ForceCompressed` 变体
- [ ] **步骤2**: `context/strategy.rs` — 修改 `auto_compress` 添加 `force` 参数，调整为更激进的压缩阈值
- [ ] **步骤3**: `context/mod.rs` — 添加 `is_blocked()`, `is_critical()`, `force_compress()` 方法
- [ ] **步骤4**: `main.rs` — 创建第二个模型客户端用于摘要器；添加阻塞检查逻辑
- [ ] **步骤5**: `cargo check` 验证编译
