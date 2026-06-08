# Context Window 管理对比分析 — 附录

> 原文拆分自 `../context-management-analysis.md`。

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
