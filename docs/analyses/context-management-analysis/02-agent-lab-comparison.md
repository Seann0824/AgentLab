# Context Window 管理对比分析 — Agent-Lab 现状与对比

> 原文拆分自 `../context-management-analysis.md`。

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

