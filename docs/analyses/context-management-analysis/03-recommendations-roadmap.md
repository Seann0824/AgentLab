# Context Window 管理对比分析 — 优化建议与路线图

> 原文拆分自 `../context-management-analysis.md`。

### 5.1 立即修复（P0 — 解决"满了不能发消息"）

#### 5.1.1 添加阻塞机制

**文件**: `src/context/mod.rs` 和 `src/main.rs`

问题: 当 Token 超过限制后，`add_message()` 继续添加，系统无阻止。

方案:
```rust
// 在 ContextManager 中添加:
pub fn is_blocked(&self) -> bool {
    let limit = self.strategy.token_limit().unwrap_or(128_000);
    self.cached_token_count >= (limit as f64 * 0.95) as usize
}

// 在 main.rs 中用户输入前检查:
if ctx.is_blocked() {
    // 必须先触发强制压缩
    eprintln!("⚠️ 上下文已满，正在强制压缩...");
    ctx.compress();
    
    // 如果压缩后仍然阻塞，阻止发送并提示
    if ctx.is_blocked() {
        eprintln!("❌ 上下文已满，请使用 /clear 或 /compact 命令");
        continue;
    }
}
```

#### 5.1.2 启用 LLM 摘要

**文件**: `src/main.rs`

当前: `ctx.setup_summary_channel(None);`
改为: `ctx.setup_summary_channel(Some(query_client.clone()));`

但这需要 `ModelAdapter` trait 实现 `Clone`。当前 `ModelAdapter` 是 `Box<dyn ModelAdapter>`，需要重构。

**临时方案**: 在 `summarizer.rs` 中实现同步 LLM 摘要（阻塞模式，在压缩前调用模型）：

```rust
pub fn llm_based_summary(
    messages: &[ContextMessage],
    model: &dyn ModelAdapter,
) -> String {
    let prompt = format!(
        "请生成详细的对话摘要，包含: 1) 用户意图 2) 关键发现 3) 代码变更 4) 决策 5) 待办...
         对话内容:\n{}",
        format_messages_for_summary(messages)
    );
    // 同步调用模型
    let response = model.sync_chat(...);
    response
}
```

### 5.2 短期优化（P1 — 提升用户体验）

#### 5.2.1 多级 Token 预警

**文件**: `src/main.rs`

```rust
// 分级预警逻辑
fn check_token_warning(stats: &ContextStats, token_limit: usize) -> Option<String> {
    let ratio = stats.usage_ratio;
    if ratio >= 0.95 {
        Some("🔴 上下文已满，即将阻塞".to_string())
    } else if ratio >= 0.85 {
        Some("🟡 上下文使用率较高，即将自动压缩".to_string())
    } else if ratio >= 0.70 {
        Some("🟢 上下文使用率正常".to_string())
    } else {
        None
    }
}
```

**输出到用户可见的地方**（不是 stderr）：

在用户输入前打印：
```
[Token: 112K/128K (87.5%)] 🟡 上下文使用率较高
>
```

#### 5.2.2 实现 /compact 命令

**文件**: `src/tools/` 新文件 `compact_tool.rs`

实现一个手动压缩命令，用户可以主动触发：
```
/compact → 立即执行四层压缩
/compact status → 显示当前 token 状态
```

### 5.3 中期优化（P2 — 达到 Claude-Code 级别）

#### 5.3.1 LLM 摘要 Prompt 升级

**文件**: `src/context/summarizer.rs`

参考 claude-code 的 `prompt.ts`，升级摘要 prompt：

```rust
const COMPACT_PROMPT: &str = r#"你的任务是对以下对话生成详细的技术摘要。

请包含以下8个方面：
1. **主要请求和意图** — 用户的所有显式请求
2. **关键技术概念** — 涉及的技术栈、框架
3. **文件和代码** — 具体文件路径、代码片段、修改原因
4. **错误和修复** — 遇到的错误及如何修复
5. **问题解决** — 已解决的问题记录
6. **所有用户消息** — 完整列出（非工具结果的消息）
7. **待办任务** — 还需要继续的工作
8. **当前工作状态** — 压缩前最后在做什么

请用 <analysis> 标签包裹分析过程，用 <summary> 标签包裹最终摘要。
"#;
```

#### 5.3.2 边界标记机制

**文件**: `src/context/types.rs` + `src/context/mod.rs`

添加边界标记消息类型，压缩后插入标记：
```rust
pub struct CompactBoundary {
    pub pre_compact_token_count: usize,
    pub post_compact_token_count: usize,
    pub summary: String,
    pub kept_turns: usize,
}
```

#### 5.3.3 Post-Compact 恢复

**文件**: `src/context/mod.rs` 或新文件 `post_compact.rs`

压缩后自动恢复关键文件读取内容：
```rust
pub fn post_compact_restore(messages: &[ContextMessage]) -> Vec<ChatMessage> {
    // 1. 从最近的 assistant 消息中提取提到的文件路径
    // 2. 重新读取这些文件（最多5个，每个最多5K tokens）
    // 3. 将读取结果作为新的 user 消息注入
}
```

#### 5.3.4 API 原生上下文管理

对于支持 `context_editing` 的 API 提供商（如 Anthropic），可以实现：

```rust
pub fn get_api_context_management(strategy: &ContextStrategy) -> Option<serde_json::Value> {
    // clear_tool_uses_20250919
    // clear_thinking_20251015
}
```

### 5.4 长期优化（P3 — 进阶功能）

#### 5.4.1 会话记忆持久化

类似 claude-code 的 `sessionMemoryCompact.ts`：
- 将关键上下文写入 `~/.agent-lab/memory/` 目录
- 新会话启动时自动加载
- 支持跨会话上下文恢复

#### 5.4.2 基于时间的微压缩

类似 `timeBasedMCConfig.ts`：
- 超过一定时间的旧工具结果自动压缩
- 配置文件可自定义过期时间

#### 5.4.3 自动回退 + 重试

类似 `autoCompact.ts` 的 `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3`：
- 连续压缩失败 3 次后停止重试
- 发出用户通知并提供手动选项

---

## 6. 优先级路线图

### Phase 1 (P0) — 解决核心问题
- [ ] **1.1 添加阻塞检测** — Token 超过 95% 时阻止发送消息
- [ ] **1.2 强制压缩机制** — 阻塞时自动触发强制压缩
- [ ] **1.3 启用 LLM 摘要** — 传入 ModelAdapter，实际调用 LLM

### Phase 2 (P1) — 提升用户体验
- [ ] **2.1 多级预警** — Token 使用率分等级显示给用户
- [ ] **2.2 /compact 命令** — 用户可手动触发压缩
- [ ] **2.3 /token 命令** — 显示 Token 使用详情
- [ ] **2.4 压缩进度显示** — 压缩过程可视

### Phase 3 (P2) — 达到业界水平
- [ ] **3.1 升级摘要 Prompt** — 9 部分结构化摘要
- [ ] **3.2 边界标记** — 插入压缩标记，保留元数据
- [ ] **3.3 Post-Compact 恢复** — 恢复关键文件读取
- [ ] **3.4 API 原生上下文管理** — 利用 API 内置能力

### Phase 4 (P3) — 进阶功能
- [ ] **4.1 会话记忆持久化** — 跨会话上下文
- [ ] **4.2 时间基微压缩** — 旧结果自动过期
- [ ] **4.3 自动重试** — 失败后优雅兜底
- [ ] **4.4 Token 预算控制** — 类似 maxBudgetUsd

---

