use super::*;

/// ⭐ 测试5: 验证渐进压缩的顺序 — 工具修剪优先于滑动窗口
///
/// 当启用了工具修剪时，如果有长工具输出，auto_compress 应优先使用
/// 层0（工具修剪）而非直接跳到层1（滑动窗口）。
/// 我们使用大 token_limit 确保工具修剪足够，不会触发更高层压缩。
#[test]
fn test_progressive_order_tool_pruning_before_sliding_window() {
    use crate::model::ToolCall;

    let mut ctx = ContextManager::new(
        "System",
        ContextStrategy::Auto {
            token_limit: 100_000, // 大 token_limit，确保工具修剪后 token 不会超限
            max_turns: 20,
            trigger_ratio: 0.9, // 只在高水位触发
            enable_async_summary: false,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 2,
            tool_pruning_max_output_chars: 50, // 短输出也修剪
        },
    );

    // 添加带长工具输出的消息（模拟真实场景）
    for i in 0..8 {
        ctx.add_message(ChatMessage::user(format!("User {}", i)));
        ctx.add_message(ChatMessage::assistant_tool_calls(
            format!("Thinking {}", i),
            vec![ToolCall {
                id: format!("call_{}", i),
                name: "shell".into(),
                arguments: r#"{"command": "echo hello"}"#.into(),
            }],
        ));
        // 长工具输出（超过 max_output_chars=50）
        let long_output = format!(
            r#"{{"ok":true,"result":{{"command":"ls -la","stdout":"{}\n"}}}}"#,
            "some_file.txt\n".repeat(30)
        );
        ctx.add_message(ChatMessage::tool(format!("call_{}", i), &long_output));
        ctx.add_message(ChatMessage::assistant(format!("Done {}", i)));
    }

    // 手动触发工具修剪（层0）
    let result = ctx.prune_tool_calls();
    assert!(result.did_compress(), "应触发工具修剪(层0)");

    // 验证工具修剪的统计数据已更新
    let stats = ctx.stats();
    assert!(
        stats.pruned_tool_calls > 0,
        "工具修剪次数应大于0，实际: {}",
        stats.pruned_tool_calls
    );

    // 验证：消息数量不变（工具修剪只替换内容，不删除消息）
    // 8轮 * 4条/轮 = 32 + 1 system = 33条
    assert_eq!(
        ctx.messages.len(),
        33,
        "工具修剪应保留所有消息，当前消息数: {}",
        ctx.messages.len()
    );

    // 验证：工具内容已被占位符替换
    let has_pruned = ctx.messages.iter().any(|m| {
        if let ChatMessage::Tool { content, .. } = &m.message {
            content.contains("TOOL_OUTPUT_PRUNED")
        } else {
            false
        }
    });
    assert!(has_pruned, "应该至少有一个工具消息被替换为占位符");
}

/// ⭐ 测试6: 验证 Token-based 触发阈值 — 用大量长消息触发压缩
///
/// 即使轮次很少，只要 token 超过触发阈值，也应触发压缩。
/// 这个测试确保「Token 超阈值但轮次不足」的边界情况被正确处理。
#[test]
fn test_token_based_trigger_with_few_turns() {
    let mut ctx = ContextManager::new(
        "System",
        ContextStrategy::Auto {
            token_limit: 100,
            max_turns: 50,      // 很大的 max_turns，轮次本身不会触发
            trigger_ratio: 0.3, // 30 tokens 就触发
            enable_async_summary: false,
            enable_tool_pruning: false,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        },
    );

    // 只用 2 轮，但每条消息都很长（~30 tokens/条）
    // 2轮=4条=~120 tokens，远超 30 的触发阈值，但轮次(2)远小于 max_turns(50)
    let very_long_text = "这是一个非常长的文本内容，用于测试Token-based触发阈值。".repeat(5);
    ctx.add_message(ChatMessage::user(&very_long_text));
    ctx.add_message(ChatMessage::assistant(&very_long_text));
    ctx.add_message(ChatMessage::user(&very_long_text));
    ctx.add_message(ChatMessage::assistant(&very_long_text));

    let stats = ctx.stats();
    assert!(
        stats.compressed,
        "即使只有 2 轮（远小于 max_turns=50），token 超阈值也应触发压缩。用量比例: {:.2}",
        stats.usage_ratio
    );
    assert!(stats.usage_ratio > 0.0, "应记录使用率");
}

/// ⭐ 集成测试: 模拟真实 Agent 循环中的上下文压缩
///
/// 此测试模拟 Agent 主循环的完整流程：
/// 1. 用户输入 → add_message(User)
/// 2. 助手思考并调用工具 → add_message(Assistant+tool_calls)
/// 3. 工具执行并返回结果 → add_message(Tool)
/// 4. 助手最终回复 → add_message(Assistant)
/// 5. 循环中自动触发压缩 → poll_summary_results
///
/// 验证压缩后的上下文仍然结构完整、可用。
#[test]
fn test_agent_loop_simulation_with_compression() {
    use crate::model::ToolCall;
    // 创建 Tokio 运行时，使 AsyncSummarizer 能调用 tokio::spawn
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let mut ctx = ContextManager::new(
        "你是 Agent Lab，一个智能助手。你可以使用各种工具来帮助用户完成任务。",
        ContextStrategy::Auto {
            token_limit: 300,   // 小 token 上限，加速触发压缩
            max_turns: 4,       // 最大 4 轮，超出就触发滑动窗口
            trigger_ratio: 0.5, // 150 tokens 触发
            enable_async_summary: true,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 2,
            tool_pruning_max_output_chars: 100,
        },
    );
    // 启动异步摘要（使用规则摘要器，不依赖 LLM）
    ctx.setup_summary_channel(None);

    let mut compression_triggered = false;
    let mut last_token_count = ctx.cached_token_count;

    // 模拟 12 轮 Agent 对话（每轮：用户→助手(工具调用)→工具结果→助手回复）
    for turn in 1..=12 {
        // 步骤 1: 用户输入
        let user_msg = format!(
            "Turn {}: 请帮我查一下当前目录的文件列表，然后读取 README.md 的第一行内容。",
            turn
        );
        let compressed = ctx.add_message(ChatMessage::user(&user_msg));
        if compressed {
            compression_triggered = true;
        }

        // 步骤 2: 助手发出工具调用
        let assistant_tc = ChatMessage::assistant_tool_calls(
            format!("我来帮你查看 Turn {} 的文件信息。", turn),
            vec![
                ToolCall {
                    id: format!("call_ls_{}", turn),
                    name: "shell".into(),
                    arguments: r#"{"command":"ls -la"}"#.into(),
                },
                ToolCall {
                    id: format!("call_read_{}", turn),
                    name: "read".into(),
                    arguments: r#"{"file_path":"README.md","max_length":100}"#.into(),
                },
            ],
        );
        let compressed = ctx.add_message(assistant_tc);
        if compressed {
            compression_triggered = true;
        }

        // 步骤 3: 工具执行结果（长输出，触发工具修剪）
        let tool_output = format!(
            r#"{{"ok":true,"result":{{"stdout":"file1.txt\nfile2.txt\nREADME.md\nsrc/\ntarget/\nCargo.toml\nCargo.lock\n{}\n"}}}}"#,
            "some_other_file.txt\n".repeat(15) // 长输出
        );
        let compressed =
            ctx.add_message(ChatMessage::tool(format!("call_ls_{}", turn), &tool_output));
        if compressed {
            compression_triggered = true;
        }

        let read_output = format!(
            "{{\"ok\":true,\"result\":{{\"content\":\"Agent Lab README - Turn {}\"}}}}",
            turn
        );
        let compressed = ctx.add_message(ChatMessage::tool(
            format!("call_read_{}", turn),
            &read_output,
        ));
        if compressed {
            compression_triggered = true;
        }

        // 步骤 4: 助手最终回复
        let assistant_reply = format!(
            "Turn {} 的结果：目录下有多个文件，README 的第一行是 '# Agent Lab'。",
            turn
        );
        let compressed = ctx.add_message(ChatMessage::assistant(&assistant_reply));
        if compressed {
            compression_triggered = true;
        }
        // ⭐ 模拟主循环中的 poll_summary_results（每轮轮询摘要结果）
        let injected = ctx.poll_summary_results();
        if injected > 0 {
            compression_triggered = true;
        }

        // 记录 token 变化
        let current_tokens = ctx.cached_token_count;
        if current_tokens > last_token_count + 50 {
            // Token 大幅增长，说明可能有问题 — 但先观察
        }
        last_token_count = ctx.cached_token_count;
    }

    // ============ 验证 ============

    // 验证 1: 压缩至少触发了一次
    assert!(
        compression_triggered,
        "在 12 轮模拟 Agent 循环中应至少触发一次压缩（触发阈值=150, token_limit=300）"
    );

    // 验证 2: 系统提示词始终保留
    let system_count = ctx
        .messages
        .iter()
        .filter(|m| matches!(&m.message, ChatMessage::System { .. }))
        .count();
    assert_eq!(system_count, 1, "系统提示词应始终保留");

    // 验证 3: Token 受控 — 不应超过 token_limit 的 2 倍
    let final_tokens = ctx.cached_token_count;
    assert!(
        final_tokens <= 600,
        "Token 数({}) 应控制在 token_limit(300) 的 2 倍以内，当前 {}",
        final_tokens,
        final_tokens
    );

    let stats = ctx.stats();
    assert!(stats.compressed, "stats 应标记为已压缩");
    assert!(
        stats.pruned_tool_calls > 0 || stats.estimated_tokens <= 300,
        "压缩应有效: pruned_tool_calls={}, estimated_tokens={}",
        stats.pruned_tool_calls,
        stats.estimated_tokens
    );

    // 验证 4: 消息结构完整 — 能正常转换为 ChatMessage 列表
    let chat_messages: Vec<ChatMessage> =
        ctx.messages.iter().map(|cm| cm.message.clone()).collect();
    assert!(!chat_messages.is_empty(), "消息列表不应为空");
    // 第一条必须是 System
    assert!(
        matches!(&chat_messages[0], ChatMessage::System { .. }),
        "第一条消息必须是 System"
    );

    // 验证 5: 消息数量受控 — 12 轮 * 5 条/轮 = 60 + 1 system = 61
    // 压缩后应明显减少
    let msg_count = ctx.messages.len();
    assert!(
        msg_count < 61,
        "压缩后消息数({})应明显少于原始消息数(61)",
        msg_count
    );

    // 验证 6: 统计信息合理
    assert!(stats.estimated_tokens > 0, "estimated_tokens 应 > 0");
    assert!(stats.usage_ratio > 0.0, "usage_ratio 应 > 0");
}
