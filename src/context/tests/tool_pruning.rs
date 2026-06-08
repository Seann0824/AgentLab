use super::*;

// ⭐ 测试手动调用工具修剪
#[test]
fn test_prune_tool_calls_manual() {
    let mut ctx = ContextManager::new(
        "System",
        ContextStrategy::Auto {
            token_limit: 100_000,
            max_turns: 20,
            trigger_ratio: 0.9,
            enable_async_summary: false,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 2,
            tool_pruning_max_output_chars: 50,
        },
    );

    // 添加一些包含长工具输出的消息
    use crate::model::ToolCall;
    for i in 0..10 {
        ctx.add_message(ChatMessage::user(format!("User {}", i)));
        ctx.add_message(ChatMessage::assistant_tool_calls(
            format!("Thinking {}", i),
            vec![ToolCall {
                id: format!("call_{}", i),
                name: "shell".into(),
                arguments: r#"{"command": "echo ok"}"#.into(),
            }],
        ));
        let long_output = format!(
            r#"{{"ok":true,"result":{{"command":"echo ok","stdout":"{}\n"}}}}"#,
            "ok".repeat(500)
        );
        ctx.add_message(ChatMessage::tool(format!("call_{}", i), &long_output));
        ctx.add_message(ChatMessage::assistant(format!("Done {}", i)));
    }

    let count_before = ctx.messages.len();

    let result = ctx.prune_tool_calls();

    assert!(result.did_compress(), "Should prune some tool calls");
    // 消息数量不变（只替换内容）
    assert_eq!(ctx.messages.len(), count_before);

    if let CompressResult::ToolCallsPruned {
        pruned_count,
        saved_tokens,
    } = &result
    {
        assert!(*pruned_count > 0, "Should have pruned some calls");
        assert!(*saved_tokens > 0, "Should have saved tokens");
        assert!(
            ctx.stats().pruned_tool_calls >= *pruned_count,
            "Stats should track pruned calls"
        );
    }
}

#[test]
fn test_prune_tool_calls_disabled() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 5 });

    let result = ctx.prune_tool_calls();
    assert!(!result.did_compress(), "SlidingWindow mode has no pruning");
}
