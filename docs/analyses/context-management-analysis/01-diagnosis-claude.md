# Context Window 管理对比分析 — 问题诊断与 Claude-Code 方案

> 原文拆分自 `../context-management-analysis.md`。

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

