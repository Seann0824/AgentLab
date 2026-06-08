# 错误排查（Error Investigation）能力 — 技术方案 — 核心场景与整体设计

> 原文拆分自 `../replay-capability.md`。

## 1. 核心场景

### 场景一：工具调用报错（主场景）

```
Agent 调用 shell("cargo build") → 编译失败 (exit code 1)
                                  ↓
自动保存快照: 当前上下文消息 + 错误信息 + 当前任务状态
                                  ↓
Agent 调用 investigate("snapshot_001")
                                  ↓
LLM 在快照上下文中分析 → 输出「哪里错、为什么错、怎么修」
```

### 场景二：spawn_agent 子进程失败

```
spawn_agent(task="fix-bug")
  → 子 agent 执行
  → 子 agent 报错退出（exit code != 0）
  → 自动保存子 agent 的上下文快照
  → 自动触发 investigate 分析
  → 返回结果自带根因分析
```

### 场景三：事后排查

```
用户: "刚才那个编译错误是怎么回事？"
Agent: 调 investigate("snapshot_002") 查看快照
       → 看到当时完整上下文 + 错误信息
       → 输出排查报告
```

---

## 2. 整体设计

### 2.1 设计原则

1. **只记录错误现场** — 不记录每一步，只在出错时自动保存快照
2. **轻量** — 一个错误快照就是一份 JSON 文件，存关键上下文片段（不是全量消息）
3. **排查即分析** — `investigate` 工具就是把快照喂给 LLM，让 LLM 在完整上下文中做根因分析
4. **自动触发** — spawn_agent 失败自动走排查流程

### 2.2 数据流

```
agent.rs 主循环
  │
  │  工具执行 ↓
  ▼
工具返回错误 (exit code != 0 / ToolEvent::Err)
  │
  │  自动 ↓
  ▼
ErrorSnapshot::capture()
  ├── 截取当前 ContextManager 的关键消息（最后 N 轮）
  ├── 记录错误信息（工具名、参数、错误输出）
  ├── 记录当时任务状态（PLAN.md / AGENDA.md 内容）
  └── 保存到 .agent/snapshots/snapshot_001.json
  │
  │  Agent 排查 ↓
  ▼
investigate("snapshot_001")
  ├── 加载快照
  ├── 构造排查 Prompt（包含：错误信息 + 上下文消息 + 任务状态）
  ├── 调用 LLM 做根因分析
  └── 输出排查报告
```

### 2.3 触发点

在 `agent.rs` 主循环中，工具执行完毕后检查结果：

```rust
// 在 agent.rs run() 中，收集完 tool_results 后
for tool_result in &tool_results {
    if is_error_result(tool_result) {
        // ⭐ 自动保存错误快照
        let snapshot = ErrorSnapshot::capture(
            &self.context_manager,
            &self.task_manager,
            tool_result,
        )?;
        snapshot.save()?;
        // 后续 Agent 可以通过 investigate 工具查看
    }
}
```

---

