
# Context Window 管理对比分析

> 分析日期: 2025-06-07
> 分析范围: agent-lab (Rust) vs claude-code (TypeScript)
> 核心问题: 上下文窗口满了后无法继续发送消息

---

## 目录

1. [问题诊断](#1-问题诊断)
2. [Claude-Code 方案详解](#2-claude-code-方案详解)
3. [Agent-Lab 现有方案](#3-agent-lab-现有方案)
4. [差异对比表](#4-差异对比表)
5. [优化建议方案](#5-优化建议方案)
6. [优先级路线图](#6-优先级路线图)

---

## 1. 问题诊断

### 1.1 当前现象

- 对话持续一段时间后，上下文窗口占满
- 满了后不能再发送消息（系统没有任何处理机制）
- 用户没有感知到 Token 使用率的预警

### 1.2 根因分析

| 问题 | 严重程度 | 说明 |
|------|---------|------|
| 触达限制后无行动 | 🔴 致命 | 当前 `add_message()` 触发压缩检查，但如果压缩后仍然超限 → 没有阻止机制 |
| LLM 摘要未启用 | 🟡 中等 | `setup_summary_channel(None)` 传入了 `None`，异步摘要退化为规则摘要 |
| 规则摘要质量差 | 🟡 中等 | `rule_based_summary()` 只提取关键词、命令、文件名，丢失语义关联 |
| 无用户可见警告 | 🟡 中等 | `eprint!` 输出到 stderr，终端用户看不到 |
| 无 auto-compact 触发 | 🔴 重要 | 只在 `add_message()` 时检查，没有轮次后独立检测 |
| 无 post-compact 恢复 | 🔵 低 | 压缩后不恢复关键文件读取结果 |

---

## 2. Claude-Code 方案详解

来源: `/Users/sean/Desktop/repo/claude-code/services/compact/`

### 2.1 整体架构

```
┌──────────────────────────────────────────────────────────┐
│                    Claude-Code Context Management          │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  1️⃣ MicroCompact (客户端级，最轻量)                      │
│     - 替换旧工具输出为占位符 "[Old tool result cleared]" │
│     - 只对特定工具做: Read, Shell, Grep, Glob, Web等     │
│     - 保留对话结构，不丢失"调用了什么工具"               │
│     - 可选: 基于时间的清理 (timeBasedMCConfig)            │
│                                                          │
│  2️⃣ API-Level Context Management (apiMicrocompact.ts)    │
│     - 利用 Anthropic API 原生能力                         │
│     - clear_tool_uses_20250919: 清除旧工具调用参数        │
│     - clear_thinking_20251015: 清除旧的 thinking 块     │
│     - 触发阈值: 180K tokens, 目标保留: 40K tokens        │
│                                                          │
│  3️⃣ Full Compact (LLM 摘要压缩，最彻底)                 │
│     - 调用 LLM 生成详细的结构化摘要                      │
│     - 插入 SystemCompactBoundaryMessage 边界标记         │
│     - 保留: 边界标记 + 摘要消息 + 保留的消息 + 附件      │
│     - 支持部分压缩 (partial compact)                     │
│                                                          │
│  4️⃣ Session Memory Compact (会话记忆压缩)                │
│     - 将会话关键上下文存入持久化文件                     │
│     - 下次会话启动时重新加载                             │
│     - 保留: 10K~40K tokens                               │
│                                                          │
│  5️⃣ Auto-Compact 自动触发引擎 (autoCompact.ts)          │
│     每个轮次后检测 token → 达到阈值自动触发              │
└──────────────────────────────────────────────────────────┘
```

### 2.2 Auto-Compact 触发流程

```
每个轮次后 → calculateTokenWarningState()

           ┌─────────────┐
           │ Token 使用量 │
           └──────┬──────┘
                  │
         ┌────────┴────────┐
         ▼                 ▼
    < warning阈值    >= warning阈值
    (无操作)              │
                  ┌───────┴───────┐
                  ▼               ▼
            < autoCompact    >= autoCompact
            阈值(显示警告)    阈值(自动触发)
                                    │
                          ┌─────────┴─────────┐
                          ▼                   ▼
                    compact成功          连续失败3次
                          │                   │
                    postCompactCleanup   停止重试
                    (恢复文件读取等)

阈值计算:
  effectiveContextWindow = contextWindow - maxOutputTokens
  autoCompactThreshold = effectiveContextWindow - 13_000 (buffer)
  warningThreshold = effectiveContextWindow - 20_000
  blockingLimit = effectiveContextWindow - 3_000
```

### 2.3 Compact Prompt 设计

Claude-Code 的摘要 prompt 非常详细（见 `prompt.ts`），包含：

```
1. 主要请求和意图 — 详细记录用户所有显式请求
2. 关键技术概念 — 框架、技术栈
3. 文件和代码段 — 具体文件名、完整代码片段、修改原因
4. 错误和修复 — 遇到的错误及修复方式
5. 问题解决 — 已解决问题的记录
6. 所有用户消息 — 完整列出（非工具结果）
7. 待办任务 — 需要继续的工作
8. 当前工作 — 压缩前正在做什么（最详细）
9. (可选) 下一步 — 与用户最新请求对齐的下一步
```

结构化的 `<analysis>` 和 `<summary>` 块:

```
<analysis>
    ...模型在此进行时间顺序分析...
</analysis>
<summary>
    ...结构化输出到上下文...
</summary>
```

### 2.4 边界标记机制

```typescript
// SystemCompactBoundaryMessage 包含:
{
  type: 'system',
  purpose: 'compact_boundary',
  compactMetadata: {
    preCompactTokenCount: number,
    postCompactTokenCount: number,
    preservedSegment?: {
      headUuid: UUID,     // 保留段第一条消息
      anchorUuid: UUID,    // 边界标记后的锚点
      tailUuid: UUID,     // 保留段最后一条消息
    }
  }
}
```

### 2.5 Post-Compact 清理

压缩完成后执行:

1. **恢复文件读取**: 最多恢复 5 个文件的读取内容（50K token 预算，每个文件最多 5K tokens）
2. **恢复技能发现**: 重新注入已知工具信息
3. **执行 hooks**: 执行 post-compact hooks
4. **清理 prompt cache**: 通知 API 缓存已失效

### 2.6 文件清单

| 文件 | 功能 |
|------|------|
| `compact.ts` | 核心压缩引擎（1705行） |
| `autoCompact.ts` | 自动触发引擎 + 阈值计算 |
| `microCompact.ts` | 客户端级工具结果轻量压缩 |
| `apiMicrocompact.ts` | API 原生上下文管理策略 |
| `sessionMemoryCompact.ts` | 会话记忆持久化压缩 |
| `prompt.ts` | LLM 摘要 prompt 模板 |
| `grouping.ts` | 按 API round 分组消息 |
| `postCompactCleanup.ts` | 压缩后恢复/清理 |
| `compactWarningState.ts` | 警告状态管理 |
| `timeBasedMCConfig.ts` | 基于时间的微压缩配置 |

---

## 3. Agent-Lab 现有方案

### 3.1 整体架构

```
┌──────────────────────────────────────────────┐
│           Agent-Lab Context Manager           │
├──────────────────────────────────────────────┤
│                                               │
│  层0: 工具调用结果修剪 (tool_call_pruning)    │
│       - 用占位符替换旧工具输出                │
│       - 保留最近的 keep_recent 轮             │
│                                               │
│  层1: 异步模型摘要 (AsyncSummarizer)          │
│       - 非阻塞派发，结果异步注入              │
│       - ⚠️ 当前未启用 LLM (传了 None)        │
│       - 退化到 rule_based_summary             │
│                                               │
│  层2: 滑动窗口 (sliding_window_compress)      │
│       - 保留最近 N 轮                         │
│       - 保护 system + preserved 消息          │
│                                               │
│  层3: 保底截断 (hard_truncate)               │
│       - 从最早的非保护消息开始删除             │
│       - 直到 Token 低于硬限制                  │
│                                               │
└──────────────────────────────────────────────┘
```

### 3.2 当前触发逻辑

```
add_message() → check_and_compress()
    ↓
auto_compress(messages)
    ↓
1. 检查 current_tokens < trigger_threshold → NotNeeded
2. 尝试层0: tool_call_pruning → 如果还超限，继续
3. 尝试层1: 派发 async summary (滑动窗口前保存上下文)
4. 执行层2: sliding_window_compress
5. 如果还超限: 层3 hard_truncate
```

### 3.3 当前代码结构

| 文件 | 功能 | 行数 |
|------|------|------|
| `context/mod.rs` | ContextManager 主体 | 1208 |
| `context/strategy.rs` | 四层压缩算法 | 930 |
| `context/summarizer.rs` | 异步摘要 + 规则摘要 | 375 |
| `context/types.rs` | 类型定义 | 281 |
| `context/config.rs` | 策略配置 | 152 |
| `context/tokenizer.rs` | Token 估算 | ~240 |

### 3.4 当前摘要 prompt（规则摘要）

```
【历史对话摘要】
── 用户意图 ──
<提取的用户意图关键词>

── 已执行操作 ──
<提取的命令输出预览>

── 关键决策 ──
<匹配"决定/选择/改为/采用"的句子>

── 当前状态 ──
<已涉及文件列表>
```

---

## 4. 差异对比表

| 维度 | Claude-Code | Agent-Lab | 差距等级 |
|------|-------------|-----------|----------|
| **摘要质量** | LLM 生成结构化摘要（8个部分+analysis/summary） | 规则关键词提取 | 🔴 严重 |
| **触发机制** | 每轮后独立检测，渐进式阈值 | 仅在 add_message 时检查 | 🔴 重要 |
| **用户警告** | warning/error/blocking 三级警告 | 仅 eprint 到 stderr | 🔴 重要 |
| **阻塞机制** | blockingLimit 硬限制，阻止发送 | 无任何阻止 | 🔴 致命 |
| **微压缩** | 客户端 + API 双层微压缩 | 仅有工具调用修剪 | 🟡 中等 |
| **API 原生支持** | clear_tool_uses/thinking 策略 | 无 | 🟡 中等 |
| **边界标记** | SystemCompactBoundaryMessage | 无 | 🟡 中等 |
| **post-compact 恢复** | 恢复文件读取+技能发现 | 无 | 🟡 中等 |
| **会话记忆持久化** | SessionMemoryCompact | 无 | 🟡 中等 |
| **摘要 prompt 设计** | 专业的 9 部分结构 | 简单关键词提取 | 🔴 严重 |
| **失败兜底** | PTL 重试 + truncateHead | 无 | 🟡 中等 |
| **Token 估算** | 精确（API 返回）+ 粗略（快速） | tiktoken-rs 估算 | ✅ 良好 |
| **缓存优化** | 增量 token 计数 | 增量 token 缓存 | ✅ 良好 |
| **异步摘要** | LLM 摘要 + 边界标记 | 异步框架但未启用 LLM | 🟡 中等 |

---

## 5. 优化建议方案

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

## 附录 A: Claude-Code 关键代码位置

| 功能 | 文件 | 关键函数 |
|------|------|---------|
| 自动触发 | `autoCompact.ts` | `shouldAutoCompact()`, `autoCompactIfNeeded()` |
| 核心压缩 | `compact.ts` | `compactConversation()`, `streamCompactSummary()` |
| 微压缩 | `microCompact.ts` | `microCompactMessages()` |
| API 策略 | `apiMicrocompact.ts` | `getAPIContextManagement()` |
| Prompt | `prompt.ts` | `getCompactPrompt()`, `getPartialCompactPrompt()` |
| 分组 | `grouping.ts` | `groupMessagesByApiRound()` |
| 清理 | `postCompactCleanup.ts` | `runPostCompactCleanup()` |
| 阈值 | `autoCompact.ts` | `getAutoCompactThreshold()` |
| 警告 | `autoCompact.ts` | `calculateTokenWarningState()` |

## 附录 B: 当前 Agent-Lab 关键代码位置

| 功能 | 文件 | 关键函数 |
|------|------|---------|
| 上下文管理器 | `context/mod.rs` | `ContextManager::add_message()` |
| 压缩策略 | `context/strategy.rs` | `auto_compress()` |
| 工具修剪 | `context/strategy.rs` | `tool_call_pruning()` |
| 滑动窗口 | `context/strategy.rs` | `sliding_window_compress()` |
| 异步摘要 | `context/summarizer.rs` | `AsyncSummarizer::start()` |
| 规则摘要 | `context/summarizer.rs` | `rule_based_summary()` |
| 配置 | `context/config.rs` | `ContextStrategy::Auto` |
| 类型 | `context/types.rs` | `CompressResult`, `ContextMessage` |
| 主循环 | `main.rs` | `loop { ... ctx.add_message() ... }` |

---

## 附录 C: 与模型 API 的交互建议

当前 API 调用链：
```
main.rs → model::openai_compatible → HTTP POST (streaming)
```

如果要支持 API 原生上下文管理（如 Anthropic 的 `clear_tool_uses`），需要在请求体中添加：

```json
{
  "model": "claude-sonnet-4-20250514",
  "messages": [...],
  "context_editing": {
    "edits": [
      {
        "type": "clear_tool_uses_20250919",
        "trigger": {"type": "input_tokens", "value": 180000},
        "keep": {"type": "tool_uses", "value": 5}
      }
    ]
  }
}
```

这需要在 `src/model/openai_compatible.rs` 中的请求体构造处添加 `context_editing` 字段。
