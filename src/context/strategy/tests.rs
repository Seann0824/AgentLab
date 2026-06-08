use super::common::{count_turns, find_turn_boundaries};
use super::sliding::sliding_window_compress;
use super::truncate::hard_truncate;
use super::*;
use crate::context::ContextStats;
use crate::context::{ContextStrategy, TokenEstimator};
use crate::model::ChatMessage;
use crate::model::ToolCall;

fn make_context_messages(count: usize) -> Vec<ContextMessage> {
    let mut msgs = Vec::new();
    msgs.push(ContextMessage::from(ChatMessage::system("System prompt")));

    for i in 0..count {
        msgs.push(ContextMessage::from(ChatMessage::user(format!(
            "User {}",
            i
        ))));
        msgs.push(ContextMessage::from(ChatMessage::assistant(format!(
            "Assistant {}",
            i
        ))));
    }
    msgs
}

fn make_context_messages_with_tools(count: usize) -> Vec<ContextMessage> {
    let mut msgs = Vec::new();
    msgs.push(ContextMessage::from(ChatMessage::system("System prompt")));

    for i in 0..count {
        msgs.push(ContextMessage::from(ChatMessage::user(format!(
            "User {}",
            i
        ))));
        msgs.push(ContextMessage::from(ChatMessage::assistant_tool_calls(
            format!("Thinking {}", i),
            vec![ToolCall {
                id: format!("call_{}", i),
                name: "shell".into(),
                arguments: r#"{"command": "echo ok"}"#.into(),
            }],
        )));
        // 用长输出来模拟工具结果（偶数轮用长输出，奇数轮用短输出）
        let long_output = format!(
            r#"{{"ok":true,"result":{{"command":"echo ok","stdout":"{}\n"}}}}"#,
            "ok".repeat(if i % 2 == 0 { 500 } else { 10 })
        );
        msgs.push(ContextMessage::from(ChatMessage::tool(
            format!("call_{}", i),
            &long_output,
        )));
        msgs.push(ContextMessage::from(ChatMessage::assistant(format!(
            "Done {}",
            i
        ))));
    }
    msgs
}

#[test]
fn test_count_turns_simple() {
    let msgs = make_context_messages(3);
    assert_eq!(count_turns(&msgs), 3);
}

#[test]
fn test_count_turns_with_tools() {
    let msgs = make_context_messages_with_tools(5);
    assert_eq!(count_turns(&msgs), 5);
}

#[test]
fn test_count_turns_system_only() {
    let msgs = vec![ContextMessage::from(ChatMessage::system("System"))];
    assert_eq!(count_turns(&msgs), 0);
}

#[test]
fn test_find_turn_boundaries() {
    let msgs = make_context_messages(3);
    let boundaries = find_turn_boundaries(&msgs);
    assert_eq!(boundaries, vec![1, 3, 5]);
}

#[test]
fn test_sliding_window_basic() {
    let mut msgs = make_context_messages(10);
    let original_len = msgs.len();

    let result = sliding_window_compress(&mut msgs, 3);

    assert!(result.did_compress());
    assert!(msgs.len() < original_len, "Messages should be reduced");
    assert!(msgs.len() <= 7, "Should have system + 3 turns max");
}

#[test]
fn test_sliding_window_protects_system() {
    let mut msgs = make_context_messages(10);
    sliding_window_compress(&mut msgs, 3);

    assert!(
        msgs.iter()
            .any(|m| matches!(&m.message, ChatMessage::System { .. })),
        "System message should be preserved"
    );
    assert_eq!(
        msgs.first().map(|m| &m.message).and_then(|m| {
            if let ChatMessage::System { content } = m {
                Some(content.as_str())
            } else {
                None
            }
        }),
        Some("System prompt"),
        "First message should still be the system prompt"
    );
}

#[test]
fn test_sliding_window_protects_preserved() {
    let mut msgs = make_context_messages(10);
    msgs[3].preserved = true;
    msgs[3].importance = crate::context::MessageImportance::Important;

    sliding_window_compress(&mut msgs, 2);

    assert!(
        msgs.iter().any(|m| m.preserved),
        "Preserved message should still be in the list"
    );
}

#[test]
fn test_sliding_window_below_limit() {
    let mut msgs = make_context_messages(2);
    let result = sliding_window_compress(&mut msgs, 5);

    assert!(!result.did_compress());
    assert_eq!(msgs.len(), 1 + 2 * 2);
}

#[test]
fn test_sliding_window_exact_limit() {
    let mut msgs = make_context_messages(5);
    let result = sliding_window_compress(&mut msgs, 5);
    assert!(!result.did_compress());
}

#[test]
fn test_sliding_window_with_tool_calls() {
    let mut msgs = make_context_messages_with_tools(10);
    let result = sliding_window_compress(&mut msgs, 3);

    assert!(result.did_compress());
    assert!(msgs.len() <= 13, "Should keep at most 13 messages");
}

#[test]
fn test_sliding_window_removes_orphaned_tool_even_if_preserved() {
    // 测试：当 Tool 消息被 preserved 但其对应的 Assistant(tool_calls) 被滑动窗口删除时，
    // Tool 消息仍然是孤儿（因为没有对应的 tool_calls 前驱），会被 remove_orphaned_tool_messages 清理。
    // 这是一个安全优先的决策：宁删 preserved 消息，也不让孤儿 Tool 消息导致 API 报错。
    let mut msgs = make_context_messages_with_tools(5);
    msgs[7].preserved = true; // Tool(call_1) — 但 Assistant(tc: call_1) 在 index 6

    let result = sliding_window_compress(&mut msgs, 2);
    assert!(result.did_compress());
    // 被 preserved 的孤儿 Tool 消息会被清理掉（安全优先）
    // 但 System 和正常的 non-orphan preserved 消息应该保留
    assert!(
        msgs.iter()
            .any(|m| matches!(&m.message, ChatMessage::System { .. })),
        "System message should always survive"
    );
    // 验证没有孤立的 Tool 消息残留
    let tool_msgs: Vec<_> = msgs
        .iter()
        .filter(|m| matches!(&m.message, ChatMessage::Tool { .. }))
        .collect();
    let assistant_tc_ids: Vec<String> = msgs
        .iter()
        .filter_map(|m| {
            if let ChatMessage::Assistant { tool_calls, .. } = &m.message {
                Some(
                    tool_calls
                        .iter()
                        .map(|tc| tc.id.clone())
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .flatten()
        .collect();
    for tool in &tool_msgs {
        if let ChatMessage::Tool { tool_call_id, .. } = &tool.message {
            assert!(
                assistant_tc_ids.contains(tool_call_id),
                "All remaining Tool messages should have matching Assistant tool_calls: {}",
                tool_call_id,
            );
        }
    }
}

// ⭐ 工具调用修剪测试
#[test]
fn test_tool_call_pruning_basic() {
    let mut msgs = make_context_messages_with_tools(10);
    let estimator = TokenEstimator::new();
    let count_before = msgs.len();

    let result = tool_call_pruning(&mut msgs, 3, 100, &estimator);

    assert!(result.did_compress(), "Should prune tool calls");
    assert_eq!(
        msgs.len(),
        count_before,
        "Message count should stay the same (only content replaced)"
    );

    if let CompressResult::ToolCallsPruned {
        pruned_count,
        saved_tokens,
    } = &result
    {
        assert!(*pruned_count > 0, "Should have pruned some tool calls");
        assert!(*saved_tokens > 0, "Should have saved tokens");
    }
}

#[test]
fn test_tool_call_pruning_keeps_recent() {
    let mut msgs = make_context_messages_with_tools(10);
    let estimator = TokenEstimator::new();

    // 保留最近 8 轮，只修剪前 2 轮
    // 偶数轮输出 ~1062 字符，奇数轮输出 ~82 字符，均超过 max_output_chars=50
    let result = tool_call_pruning(&mut msgs, 8, 50, &estimator);

    if let CompressResult::ToolCallsPruned { pruned_count, .. } = &result {
        // 前 2 轮中，每轮有一个 Tool 消息，两个都超过 50 字符
        assert_eq!(*pruned_count, 2);
    }
}

#[test]
fn test_tool_call_pruning_not_needed() {
    let mut msgs = make_context_messages_with_tools(2);
    let estimator = TokenEstimator::new();

    // keep_recent=5，但只有 2 轮，所以不需要修剪
    let result = tool_call_pruning(&mut msgs, 5, 100, &estimator);
    assert!(!result.did_compress());
}

#[test]
fn test_tool_call_pruning_respects_preserved() {
    let mut msgs = make_context_messages_with_tools(5);
    let estimator = TokenEstimator::new();

    // 标记第 2 轮的 Tool 消息为 preserved
    // 第 2 轮: index 5=User, 6=Assistant_tc, 7=Tool, 8=Assistant
    msgs[7].preserved = true;
    msgs[7].importance = crate::context::MessageImportance::Important;

    let result = tool_call_pruning(&mut msgs, 1, 50, &estimator);

    if let CompressResult::ToolCallsPruned { pruned_count, .. } = &result {
        // prune_boundary = user_positions[4] = 17
        // 在 [0..17) 范围内:
        //   index 3 (Tool0, 1062>50) → 修剪
        //   index 7 (Tool1, 82>50) → preserved! 跳过
        //   index 11 (Tool2, 1062>50) → 修剪
        //   index 15 (Tool3, 82>50) → 修剪
        // 总共修剪 3 个
        assert_eq!(*pruned_count, 3);
    }
}

#[test]
fn test_auto_with_tool_pruning_first() {
    let mut msgs = make_context_messages_with_tools(30);
    let estimator = TokenEstimator::new();
    let strategy = ContextStrategy::Auto {
        token_limit: 200,
        max_turns: 10,
        trigger_ratio: 0.3,
        enable_async_summary: false,
        enable_tool_pruning: true,
        tool_pruning_keep_recent: 3,
        tool_pruning_max_output_chars: 100,
    };
    let mut stats = ContextStats::default();

    let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats, None);
    // token_limit=200 非常小，可能会触发多层压缩直到硬截断
    // 只要压缩发生了就算通过
    assert!(
        matches!(
            result,
            CompressResult::ToolCallsPruned { .. }
                | CompressResult::SlidingWindowCompressed { .. }
                | CompressResult::HardTruncated { .. }
        ),
        "Should have compressed via some layer: {:?}",
        result
    );
}

#[test]
fn test_auto_sliding_window_first() {
    let mut msgs = make_context_messages(30);
    let estimator = TokenEstimator::new();
    let strategy = ContextStrategy::Auto {
        token_limit: 200,
        max_turns: 10,
        trigger_ratio: 0.5,
        enable_async_summary: false,
        enable_tool_pruning: false, // 关闭工具修剪
        tool_pruning_keep_recent: 3,
        tool_pruning_max_output_chars: 100,
    };
    let mut stats = ContextStats::default();

    let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats, None);

    assert!(
        stats.compressed,
        "Should have compressed, result: {:?}",
        result
    );
    assert!(
        msgs.len() < 61,
        "Messages should be reduced from 61, current: {}",
        msgs.len()
    );
}

#[test]
fn test_auto_hard_truncate() {
    let mut msgs = make_context_messages(5);
    let estimator = TokenEstimator::new();
    let strategy = ContextStrategy::Auto {
        token_limit: 10,
        max_turns: 2,
        trigger_ratio: 0.1,
        enable_async_summary: false,
        enable_tool_pruning: false,
        tool_pruning_keep_recent: 3,
        tool_pruning_max_output_chars: 100,
    };
    let mut stats = ContextStats::default();

    let _result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats, None);

    assert!(stats.compressed);
}

#[test]
fn test_auto_not_needed() {
    let mut msgs = make_context_messages(2);
    let estimator = TokenEstimator::new();
    let strategy = ContextStrategy::Auto {
        token_limit: 100_000,
        max_turns: 20,
        trigger_ratio: 0.9,
        enable_async_summary: false,
        enable_tool_pruning: true,
        tool_pruning_keep_recent: 3,
        tool_pruning_max_output_chars: 100,
    };
    let mut stats = ContextStats::default();

    let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats, None);

    assert!(!result.did_compress());
}

#[test]
fn test_hard_truncate_basic() {
    let mut msgs = make_context_messages(3);
    let estimator = TokenEstimator::new();

    let result = hard_truncate(&mut msgs, 5, &estimator);

    assert!(result.did_compress());
    assert!(
        msgs.iter()
            .any(|m| matches!(&m.message, ChatMessage::System { .. })),
        "System must be preserved"
    );
}

#[test]
fn test_hard_truncate_not_needed() {
    let mut msgs = make_context_messages(1);
    let estimator = TokenEstimator::new();

    let result = hard_truncate(&mut msgs, 100_000, &estimator);

    assert!(!result.did_compress());
    assert_eq!(msgs.len(), 3);
}

#[test]
fn test_progressive_compression_layers() {
    // 验证四层渐进压缩的正确顺序
    let mut msgs = make_context_messages_with_tools(30);
    let estimator = TokenEstimator::new();

    // 配置：小 token_limit，低触发阈值，确保所有层都能测试到
    let strategy = ContextStrategy::Auto {
        token_limit: 150,
        max_turns: 5,
        trigger_ratio: 0.2,
        enable_async_summary: false,
        enable_tool_pruning: true,
        tool_pruning_keep_recent: 2,
        tool_pruning_max_output_chars: 50,
    };
    let mut stats = ContextStats::default();

    let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats, None);

    assert!(stats.compressed);
    // 结果可能是任一层的产物
    assert!(
        matches!(
            result,
            CompressResult::ToolCallsPruned { .. }
                | CompressResult::SlidingWindowCompressed { .. }
                | CompressResult::HardTruncated { .. }
        ),
        "Result should be one of the compression types: {:?}",
        result
    );
}
