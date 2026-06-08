use super::*;

// ============ 验证上下文压缩能力的新测试 ============

/// ⭐ 测试1: Token 缓存增量更新 vs 全量重算的一致性
///
/// 验证：每次 add_message 时的增量累加（estimate_message + 累加）
/// 与全量重算（estimate_messages）的结果一致。
/// 这是上下文压缩能力的基础——只有缓存准确，压缩决策才能正确。
#[test]
fn test_token_cache_incremental_vs_full_consistency() {
    let mut ctx = ContextManager::new(
        "System prompt for testing",
        ContextStrategy::Auto {
            token_limit: 100_000,
            max_turns: 20,
            trigger_ratio: 0.9,
            enable_async_summary: false,
            enable_tool_pruning: false,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        },
    );

    // 1. 初始状态：只有 system 消息
    let initial_cache = ctx.cached_token_count;
    ctx.recalculate_token_cache();
    assert_eq!(
        ctx.cached_token_count, initial_cache,
        "初始状态：增量缓存应与全量重算一致"
    );

    // 2. 逐步添加消息，每次验证增量缓存与全量重算一致
    let messages_to_add = [
        ChatMessage::user("User 1: 请列出当前目录下的所有文件"),
        ChatMessage::assistant(
            "以下是当前目录下的文件列表：\n1. src/\n2. Cargo.toml\n3. README.md",
        ),
        ChatMessage::user("User 2: 让我读取一下 Cargo.toml 的内容"),
        ChatMessage::assistant(
            "好的，我来读取 Cargo.toml 的内容。这个文件定义了我们项目的依赖和配置。",
        ),
        ChatMessage::user("User 3: 请编译并运行项目"),
        ChatMessage::assistant("正在编译项目，请稍等...编译成功！所有测试通过。"),
        ChatMessage::user("User 4: 让我检查一下代码的模块结构"),
        ChatMessage::assistant(
            "项目的模块结构如下：\n- src/main.rs：主入口\n- src/context/：上下文管理\n  - mod.rs：ContextManager\n  - strategy.rs：压缩策略\n  - types.rs：数据类型",
        ),
        ChatMessage::user("User 5: 请运行测试"),
        ChatMessage::assistant("正在运行测试...所有 75 个测试全部通过！"),
    ];

    for (i, msg) in messages_to_add.iter().enumerate() {
        ctx.add_message(msg.clone());

        // 每次添加后，重新计算全量并对比
        let incremental = ctx.cached_token_count;
        // 先让缓存失效再重算，得到全量值
        ctx.cache_valid = false;
        ctx.recalculate_token_cache();
        let full_recalc = ctx.cached_token_count;

        assert_eq!(
            incremental,
            full_recalc,
            "消息 #{} 添加后：增量缓存({})应与全量重算({})一致",
            i + 1,
            incremental,
            full_recalc
        );

        // 恢复增量缓存有效状态
        ctx.cache_valid = true;
        ctx.cached_token_count = incremental;
    }
}

/// ⭐ 测试2: 验证动态有效 max_turns 的触发逻辑
///
/// 配置小 token_limit + 低 trigger_ratio，使 auto_compress 在轮次少时
/// 也能因 token 超阈值而触发滑动窗口压缩。
#[test]
fn test_dynamic_max_turns_triggers_compression_early() {
    let mut ctx = ContextManager::new(
        "System",
        ContextStrategy::Auto {
            token_limit: 80,    // 很小的 token 限制
            max_turns: 20,      // 最大 20 轮
            trigger_ratio: 0.5, // 40 tokens 触发
            enable_async_summary: false,
            enable_tool_pruning: false,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        },
    );

    // 添加几条较长消息让 token 快速超过阈值
    // 每条消息约 15-20 tokens，4 轮（8条消息）即可超过 40 阈值
    for i in 0..8 {
        let long_user = format!(
            "User {}: 这是一个较长的用户输入，用于测试动态 max_turns 在 token 超阈值时的触发逻辑",
            i
        );
        let long_assistant = format!(
            "Assistant {}: 这是一个较长的助手回复，包含一些技术细节和代码示例",
            i
        );
        ctx.add_message(ChatMessage::user(&long_user));
        ctx.add_message(ChatMessage::assistant(&long_assistant));
    }

    let stats = ctx.stats();
    assert!(
        stats.compressed,
        "在 token 超阈值但轮次(8)远未达到 max_turns(20) 时，应触发压缩。用量比例: {:.2}",
        stats.usage_ratio
    );

    // 验证消息数量被压缩
    let turns_in_ctx = ctx
        .messages
        .iter()
        .filter(|m| matches!(&m.message, ChatMessage::User { .. }))
        .count();
    assert!(
        turns_in_ctx < 8,
        "压缩后轮次({})应少于原始轮次(8)",
        turns_in_ctx
    );
}

/// ⭐ 测试3: 验证异步摘要注入删除原文后 token 真实下降
///
/// 通过模拟摘要结果，验证 inject_summary 正确删除被摘要的原始消息，
/// 且 token 计数显著下降。
#[test]
fn test_inject_summary_reduces_tokens() {
    let mut ctx = ContextManager::new(
        "System",
        ContextStrategy::Auto {
            token_limit: 100_000,
            max_turns: 20,
            trigger_ratio: 0.9,
            enable_async_summary: false,
            enable_tool_pruning: false,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        },
    );

    // 添加 6 轮对话
    for i in 0..6 {
        ctx.add_message(ChatMessage::user(format!("User message number {}", i)));
        ctx.add_message(ChatMessage::assistant(format!(
            "Assistant response with some details for message {}",
            i
        )));
    }

    let tokens_before_inject = ctx.cached_token_count;
    let msg_count_before = ctx.messages.len();

    // 注入摘要（模拟异步摘要的结果）：summarized_count = 4 表示摘要覆盖了前 4 条非系统消息
    let summary_msg = ContextMessage {
        message: ChatMessage::assistant("【摘要】用户询问了文件列表、读取文件内容、编译项目等操作"),
        preserved: true,
        importance: MessageImportance::Important,
    };
    ctx.inject_summary(summary_msg, 4);

    // 验证消息数减少
    assert!(
        ctx.messages.len() < msg_count_before,
        "注入摘要后消息数({})应少于注入前({})",
        ctx.messages.len(),
        msg_count_before
    );

    // 验证 token 总数下降
    let tokens_after = ctx.cached_token_count;
    assert!(
        tokens_after < tokens_before_inject,
        "注入摘要后 token({})应少于注入前({})",
        tokens_after,
        tokens_before_inject
    );

    // 验证摘要消息已插入
    let has_summary = ctx.messages.iter().any(|m| {
        if let ChatMessage::Assistant { content, .. } = &m.message {
            content.contains("【摘要】")
        } else {
            false
        }
    });
    assert!(has_summary, "摘要消息应存在于上下文中");
}

/// ⭐ 测试4: 验证 end-to-end ContextManager 全生命周期
///
/// 模拟完整场景：多次 add_message → add_message 内部触发 check_and_compress
/// → 压缩后消息变少 → token 下降 → 验证统计信息正确
#[test]
fn test_end_to_end_compression_lifecycle() {
    let mut ctx = ContextManager::new(
        "System prompt",
        ContextStrategy::Auto {
            token_limit: 100,   // 很小的 token 上限
            max_turns: 5,       // 最大 5 轮
            trigger_ratio: 0.4, // 40 tokens 触发
            enable_async_summary: false,
            enable_tool_pruning: false,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        },
    );

    // 记录初始状态
    assert!(!ctx.stats().compressed, "初始状态不应压缩");
    let initial_token_count = ctx.cached_token_count;
    assert!(initial_token_count > 0, "System prompt 应有 token");

    // 阶段 1: 逐轮添加消息，跟踪压缩触发时机
    let mut compressed_at_turn: Option<usize> = None;

    for turn in 1..=20 {
        let user_msg = format!(
            "Turn {}: 用户输入一些较长的文本内容，让 token 数量逐步增长以触发压缩",
            turn
        );
        let assistant_msg = format!(
            "Turn {}: 助手的回复也包含一些内容，确保每轮都有足够的 token 消耗",
            turn
        );

        let compressed = ctx.add_message(ChatMessage::user(&user_msg));
        if compressed && compressed_at_turn.is_none() {
            compressed_at_turn = Some(turn);
        }

        let compressed = ctx.add_message(ChatMessage::assistant(&assistant_msg));
        if compressed && compressed_at_turn.is_none() {
            compressed_at_turn = Some(turn);
        }
    }

    // 验证：压缩已被触发
    assert!(
        compressed_at_turn.is_some(),
        "在 20 轮对话中应至少触发一次压缩"
    );

    // 验证：压缩后的 token 应在 token_limit 附近
    let stats = ctx.stats();
    assert!(stats.compressed, "最终 stats 应标记为已压缩");
    let final_tokens = ctx.cached_token_count;
    assert!(
        final_tokens <= 150, // 允许略微超出 token_limit
        "压缩后 token({}) 应接近或低于 token_limit(100)",
        final_tokens
    );

    // 验证：消息数量受控
    let total_msgs = ctx.messages.len();
    assert!(
        total_msgs < 41, // 20轮 * 2条/轮 = 40条，压缩后应明显减少
        "压缩后消息数({})应远小于原始消息数(40+)",
        total_msgs
    );

    // 验证：系统提示词始终保留
    let system_count = ctx
        .messages
        .iter()
        .filter(|m| matches!(&m.message, ChatMessage::System { .. }))
        .count();
    assert_eq!(system_count, 1, "系统提示词应始终保留");
}
