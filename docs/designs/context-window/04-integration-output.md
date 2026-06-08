# 功能特性设计：上下文窗口管理 (Context Window Management) — 系统集成与终端输出

> 原文拆分自 `../context-window.md`。

### 3.6 系统集成（与 main.rs 的集成）

```rust
// ⭐ 优化后的 main.rs 集成

use crate::context::ContextManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let query_client = initial_model()?;
    let tool_manager = initial_tool_manager();

    let policy_summary = /* 权限摘要 */;

    // 使用 ContextManager 替代 Vec<ChatMessage>
    let mut ctx = ContextManager::new(
        format!(
            "你当前工作的目录为 ...。\n\n{}",
            policy_summary
        ),
        ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,
            enable_async_summary: true,
        },
    );

    // 启动异步摘要后台任务
    let (summary_tx, summary_handle) = if let ContextStrategy::Auto { enable_async_summary: true, .. } = ctx.strategy() {
        let (tx, handle) = AsyncSummarizer::start(query_client.clone());
        (Some(tx), Some(handle))
    } else {
        (None, None)
    };
    ctx.set_summary_channel(summary_tx.clone());

    let mut is_auto = false;
    let mut terminal_line_dirty = false;

    loop {
        if !is_auto {
            let mut user_input = String::new();
            finish_terminal_line(&mut terminal_line_dirty);
            print!(">");
            std::io::Write::flush(&mut std::io::stdout())?;
            if std::io::stdin().read_line(&mut user_input).is_err() {
                continue;
            }
            if user_input.trim().is_empty() {
                continue;
            }
            ctx.add_message(ChatMessage::user(user_input));
        }

        // 显示当前的 Token 使用状态
        let stats = ctx.stats().clone();
        if stats.usage_ratio > 0.5 {
            // ⭐ 使用 eprint! 输出到 stderr，避免被 Shell 工具捕获
            eprint!(
                "\r\x1b[2K[Token: {}/{} ({:.0}%) | 保留 {} 条重要消息] ",
                TokenEstimator::format_tokens(stats.estimated_tokens),
                TokenEstimator::format_tokens(128_000),
                stats.usage_ratio * 100.0,
                stats.preserved_count,
            );
        }

        // 检查是否有异步摘要结果需要注入
        if let Some(ref mut rx) = ctx.summary_result_rx {
            while let Ok(summary_msg) = rx.try_recv() {
                ctx.inject_summary(summary_msg);
                // ⭐ 使用 eprint! 通知用户
                eprintln!("\r\x1b[2K📋 异步摘要已生成并注入上下文");
            }
        }

        let mut stream_chat = query_client.stream_chat(
            ctx.get_messages(),
            tool_manager.get_tools_scehma(),
        );

        // ... 后续保持不变
    }
}
```

#### 3.6.1 系统提示词补充

```rust
// 系统提示词中追加上下文管理说明
// ⭐ 区分 stdout 和 stderr 的解释

let system_prompt = format!(
    r#"你当前工作的目录为 {}。

{}  // 权限摘要

【上下文管理说明】
- 为了管理上下文窗口，早期对话历史可能会被自动压缩为摘要。
- 摘要会按「目标 → 操作 → 决策 → 状态」的结构保留关键信息。
- 如果发现某些上下文缺失，请基于摘要信息继续工作。
- 重要的上下文信息请**写入文件**，而不是仅依赖对话历史。
- 系统状态信息（如 Token 使用率）会输出到 stderr，不会混入你的工具执行结果。

【工作原则】
- 读取文件内容后，关键信息应记录在文件中，不要仅依赖对话记忆。
- 如果需要在多轮对话中保持状态，请使用文件持久化。"#,
    current_dir,
    policy_summary,
);
```

---

### 3.7 ⭐ 终端输出规范：stdout vs stderr 分离

这是原始方案中遗漏的重要问题：**Token 状态信息如果输出到 stdout，会被 Shell 工具捕获，模型下次执行命令时会看到这些日志，造成困惑**。

```rust
// ✅ 正确做法：
// - 用户交互（">" 提示符、模型输出）→ stdout
// - 系统状态（Token 使用率、压缩通知、摘要完成通知）→ stderr

// 终端显示效果（stderr 输出，不影响 stdout 的纯净性）：
//
// stderr: [Token: 23.5K/128K (18%) | 保留 2 条重要消息]
// stdout: > 帮我修改 src/main.rs
// stdout: 我来查看一下文件内容...
// stderr: ─── 🔧 调用工具: shell ───
// stderr:   $ cat src/main.rs
// stdout: (cat 命令的输出)
```

---

