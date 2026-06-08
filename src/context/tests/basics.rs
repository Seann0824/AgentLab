use super::*;

#[test]
fn test_context_manager_new() {
    let ctx = ContextManager::new(
        "System prompt",
        ContextStrategy::SlidingWindow { max_turns: 5 },
    );

    assert_eq!(ctx.messages.len(), 1);
    assert!(matches!(
        &ctx.messages[0].message,
        ChatMessage::System { .. }
    ));
    assert!(ctx.cached_token_count > 0);
    assert!(ctx.cache_valid);
}

#[test]
fn test_add_message_increments_cache() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 10 });

    let initial_tokens = ctx.cached_token_count;

    ctx.add_message(ChatMessage::user("Hello"));
    assert!(
        ctx.cached_token_count > initial_tokens,
        "Cache should increase after adding message"
    );

    ctx.add_message(ChatMessage::assistant("Hi there!"));
    assert!(
        ctx.cached_token_count > initial_tokens,
        "Cache should increase after adding assistant message"
    );
}

#[test]
fn test_add_message_triggers_compression() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 3 });

    // 添加超过窗口大小的消息（每轮2条，所以要加超过6条消息）
    for i in 0..10 {
        ctx.add_message(ChatMessage::user(format!("User {}", i)));
        ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
    }

    let stats = ctx.stats();
    assert!(stats.compressed, "Compression should have been triggered");
    assert!(
        ctx.messages.len() < 21,
        "Messages should be compressed: {}",
        ctx.messages.len()
    );
}

#[test]
fn test_system_prompt_protected() {
    let mut ctx = ContextManager::new(
        "System prompt that should never be removed",
        ContextStrategy::SlidingWindow { max_turns: 1 },
    );

    for i in 0..20 {
        ctx.add_message(ChatMessage::user(format!("User {}", i)));
        ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
    }

    let messages = ctx.get_messages();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, ChatMessage::System { .. }))
    );
    // System 应该是第一条
    if let ChatMessage::System { content } = &messages[0] {
        assert!(content.contains("System prompt"));
    } else {
        panic!("First message should be System");
    }
}

#[test]
fn test_preserve_last_message() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 2 });

    ctx.add_message(ChatMessage::user("User 1"));
    ctx.add_message(ChatMessage::assistant("Assistant 1"));

    let preserved = ctx.preserve_last_message();
    assert!(preserved, "Should be able to preserve last message");

    // 继续添加更多消息触发压缩
    for i in 2..10 {
        ctx.add_message(ChatMessage::user(format!("User {}", i)));
        ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
    }

    // preserved 消息应该还在
    assert!(
        ctx.messages.iter().any(|m| m.preserved),
        "Preserved message should survive compression"
    );
}

#[test]
fn test_get_messages_returns_chat_messages() {
    let ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 5 });

    let messages = ctx.get_messages();
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], ChatMessage::System { .. }));
}

#[test]
fn test_stats_updated() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 5 });

    ctx.add_message(ChatMessage::user("Hello"));
    ctx.add_message(ChatMessage::assistant("World"));

    let stats = ctx.stats();
    assert_eq!(stats.message_count, 3);
    assert!(stats.estimated_tokens > 0);
}

#[test]
fn test_max_preserved_limit() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 10 });
    ctx.set_max_preserved(2);

    for i in 0..5 {
        ctx.add_message(ChatMessage::user(format!("User {}", i)));
        ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
        ctx.preserve_last_message();
    }

    let preserved_count = ctx.messages.iter().filter(|m| m.preserved).count();
    assert!(
        preserved_count <= 2,
        "Should not exceed max_preserved limit: {}",
        preserved_count
    );
}

#[test]
fn test_poll_summary_no_results() {
    let mut ctx = ContextManager::new(
        "System",
        ContextStrategy::Auto {
            token_limit: 100_000,
            max_turns: 20,
            trigger_ratio: 0.7,
            enable_async_summary: false,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        },
    );

    let injected = ctx.poll_summary_results();
    assert_eq!(injected, 0);
}

#[test]
fn test_recalculate_token_cache() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 10 });

    ctx.add_message(ChatMessage::user("Hello"));
    let cache_before = ctx.cached_token_count;

    // 模拟缓存失效
    ctx.cache_valid = false;
    ctx.recalculate_token_cache();

    assert!(ctx.cache_valid);
    assert_eq!(ctx.cached_token_count, cache_before);
}

#[test]
fn test_inject_summary_triggers_compress() {
    let mut ctx = ContextManager::new("System", ContextStrategy::SlidingWindow { max_turns: 2 });

    // 加满消息
    for i in 0..5 {
        ctx.add_message(ChatMessage::user(format!("User {}", i)));
        ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
    }

    let count_before = ctx.messages.len();

    // 注入一个摘要消息
    let summary = ContextMessage {
        message: ChatMessage::user("【摘要】这是历史对话摘要"),
        preserved: true,
        importance: MessageImportance::Important,
    };
    // ⭐ 测试手动注入摘要，summarized_count=0 表示不删除原始消息
    ctx.inject_summary(summary, 0);

    // 注入后消息应该没爆炸（压缩检查应已触发）
    assert!(
        ctx.messages.len() <= count_before + 2,
        "After injection + compression, message count should be bounded: {}",
        ctx.messages.len()
    );
}
