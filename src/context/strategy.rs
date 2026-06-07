use std::time::Instant;

use crate::model::ChatMessage;

use super::config::ContextStrategy;
use super::tokenizer::TokenEstimator;
use super::types::{CompressResult, ContextMessage, ContextStats};

/// ⭐ 对话轮次计数器（统一逻辑）
///
/// 一轮的定义：以 User 消息为起始，到下一个 User 消息之前（或末尾）结束。
/// System 消息不计入轮次。
///
/// 示例：
///   System, User1, Assistant1(tc), Tool1, Assistant1(ok), User2, Assistant2
///   → 2 turns (User1 + User2)
fn count_turns(messages: &[ContextMessage]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(&m.message, ChatMessage::User { .. }))
        .count()
}

/// ⭐ 找到每轮对话的消息范围
///
/// 返回每个 User 消息的索引位置，这些就是每轮的起始边界。
fn find_turn_boundaries(messages: &[ContextMessage]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(&m.message, ChatMessage::User { .. }))
        .map(|(i, _)| i)
        .collect()
}

/// ⭐ 层1：滑动窗口压缩（同步，永不失败）
///
/// 保留规则（按优先级）：
/// 1. 系统提示词 → 始终保留
/// 2. preserved = true 的消息 → 永久保留
/// 3. 最近 N 轮对话 → 保留
///
/// 实现方式：构建新列表，而不是原地删除，避免索引管理问题。
fn sliding_window_compress(
    messages: &mut Vec<ContextMessage>,
    max_turns: usize,
) -> CompressResult {
    let turns = count_turns(messages);
    if turns <= max_turns {
        return CompressResult::NotNeeded;
    }

    let remove_turns = turns - max_turns;
    let original_len = messages.len();

    // 找到所有 User 消息的位置（轮次边界）
    let user_positions = find_turn_boundaries(messages);

    // 第 remove_turns 个 User 消息（0-indexed）是第一个要保留的轮次起点
    let keep_start = user_positions[remove_turns];

    // 构建新消息列表
    let mut new_messages: Vec<ContextMessage> = Vec::with_capacity(messages.len());

    // 保留 keep_start 之前的所有 protected 消息（System + preserved）
    for i in 0..keep_start {
        if matches!(&messages[i].message, ChatMessage::System { .. }) || messages[i].preserved {
            new_messages.push(messages[i].clone());
        }
    }

    // 保留从 keep_start 开始的所有消息
    new_messages.extend_from_slice(&messages[keep_start..]);

    let removed_count = original_len - new_messages.len();
    *messages = new_messages;

    CompressResult::SlidingWindowCompressed {
        removed_count,
        removed_turns: remove_turns,
    }
}

/// ⭐ 层3：保底截断（最后的安全网）
///
/// 触发条件：滑动窗口执行后 Token 仍然超过硬限制
/// 从最早的非保护消息开始截断，直到 Token 低于安全线
///
/// ⭐ 优化：预计算所有消息的 token 数，避免 O(n²) 循环中反复全量估算
fn hard_truncate(
    messages: &mut Vec<ContextMessage>,
    token_limit: usize,
    estimator: &TokenEstimator,
) -> CompressResult {
    let original_len = messages.len();

    // ⭐ 预计算每条消息的 token 数（一次性 O(n)）
    let token_counts: Vec<usize> = messages
        .iter()
        .map(|m| estimator.estimate_message(&m.message))
        .collect();

    let total_tokens: usize = token_counts.iter().sum();
    if total_tokens <= token_limit {
        return CompressResult::NotNeeded;
    }

    // 从最早的非保护消息开始标记删除
    let mut removed_count = 0;
    let mut tokens_remaining = total_tokens;
    let mut remove_up_to: Option<usize> = None;

    for i in 0..messages.len() {
        if tokens_remaining <= token_limit {
            break;
        }

        let is_protected =
            matches!(&messages[i].message, ChatMessage::System { .. }) || messages[i].preserved;
        if !is_protected {
            tokens_remaining = tokens_remaining.saturating_sub(token_counts[i]);
            removed_count += 1;
            remove_up_to = Some(i + 1);
        }
    }

    // 如果没删任何消息，返回 NotNeeded
    let Some(end) = remove_up_to else {
        return CompressResult::NotNeeded;
    };

    // 构建新列表：保留 protected 消息 + 剩余消息
    let mut new_messages: Vec<ContextMessage> = Vec::with_capacity(messages.len() - removed_count);

    // 保留被移除范围中的 protected 消息
    for i in 0..end {
        let is_protected =
            matches!(&messages[i].message, ChatMessage::System { .. }) || messages[i].preserved;
        if is_protected {
            new_messages.push(messages[i].clone());
        }
    }

    // 保留剩余消息
    new_messages.extend_from_slice(&messages[end..]);

    *messages = new_messages;

    CompressResult::HardTruncated {
        removed_count,
        kept_count: messages.len(),
    }
}

/// ⭐ 自动模式压缩（三层模型）
///
/// 层1: 滑动窗口（同步，O(1)，永远不会失败）
/// 层2: 异步摘要（由 ContextManager 派发，不在此处理）
/// 层3: 保底截断（同步，极端情况下使用）
pub fn auto_compress(
    messages: &mut Vec<ContextMessage>,
    strategy: &ContextStrategy,
    estimator: &TokenEstimator,
    stats: &mut ContextStats,
) -> CompressResult {
    // ⭐ 使用 if let 安全解构，而不是可能静默失败的 let-else
    let (
        token_limit,
        max_turns,
        trigger_ratio,
        _enable_async_summary,
    ) = match strategy {
        ContextStrategy::Auto {
            token_limit,
            max_turns,
            trigger_ratio,
            enable_async_summary,
        } => (*token_limit, *max_turns, *trigger_ratio, *enable_async_summary),
        // 如果传入了非 Auto 策略，记录警告并返回
        other => {
            eprintln!(
                "[WARN] auto_compress called with non-Auto strategy: {:?}",
                other
            );
            return CompressResult::NotNeeded;
        }
    };

    let current_tokens = estimate_total_tokens(messages, estimator);
    let trigger_threshold = (token_limit as f64 * trigger_ratio) as usize;

    stats.estimated_tokens = current_tokens;
    stats.usage_ratio = current_tokens as f64 / token_limit as f64;

    // 情况 A: Token 远低于阈值 → 无需操作
    if current_tokens < trigger_threshold {
        return CompressResult::NotNeeded;
    }

    // 情况 B: Token 超过阈值 → 先执行滑动窗口（一定成功）
    let turns = count_turns(messages);
    if turns > max_turns {
        let result = sliding_window_compress(messages, max_turns);
        if matches!(&result, CompressResult::SlidingWindowCompressed { .. }) {
            stats.compressed = true;
            stats.last_compressed_at = Some(Instant::now());
            return result;
        }
    }

    // 情况 C: 滑动窗口后仍然超限（极端情况）→ 保底截断
    let tokens_after = estimate_total_tokens(messages, estimator);
    if tokens_after > token_limit {
        let result = hard_truncate(messages, token_limit, estimator);
        if matches!(&result, CompressResult::HardTruncated { .. }) {
            stats.compressed = true;
            stats.last_compressed_at = Some(Instant::now());
        }
        return result;
    }

    CompressResult::NotNeeded
}

/// ⭐ 滑动窗口模式压缩（外部调用入口）
pub fn sliding_window_mode(
    messages: &mut Vec<ContextMessage>,
    max_turns: usize,
    stats: &mut ContextStats,
) -> CompressResult {
    let result = sliding_window_compress(messages, max_turns);
    if matches!(&result, CompressResult::SlidingWindowCompressed { .. }) {
        stats.compressed = true;
        stats.last_compressed_at = Some(Instant::now());
    }
    result
}

/// 计算消息列表的总 token 数
fn estimate_total_tokens(messages: &[ContextMessage], estimator: &TokenEstimator) -> usize {
    let raw: Vec<ChatMessage> = messages.iter().map(|m| m.message.clone()).collect();
    estimator.estimate_messages(&raw)
}

// ================= 导出的辅助函数（供外部测试使用） =================

#[doc(hidden)]
pub fn _test_count_turns(messages: &[ContextMessage]) -> usize {
    count_turns(messages)
}

#[doc(hidden)]
pub fn _test_sliding_window_compress(
    messages: &mut Vec<ContextMessage>,
    max_turns: usize,
) -> CompressResult {
    sliding_window_compress(messages, max_turns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextStats;
    use crate::model::ChatMessage;

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
                vec![crate::model::ToolCall {
                    id: format!("call_{}", i),
                    name: "shell".into(),
                    arguments: r#"{"command": "echo ok"}"#.into(),
                }],
            )));
            msgs.push(ContextMessage::from(ChatMessage::tool(
                format!("call_{}", i),
                r#"{"ok": true, "result": {"stdout": "ok\n"}}"#,
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
        // 每个完整的轮次只计 1 个 User 消息
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
        // System 在 0, User1 在 1, Assistant1 在 2, User2 在 3, ...
        assert_eq!(boundaries, vec![1, 3, 5]);
    }

    #[test]
    fn test_sliding_window_basic() {
        let mut msgs = make_context_messages(10);
        let original_len = msgs.len();

        let result = sliding_window_compress(&mut msgs, 3);

        assert!(matches!(result, CompressResult::SlidingWindowCompressed { .. }));
        assert!(msgs.len() < original_len, "Messages should be reduced");
        // System 消息 + 3 轮对话（每轮 2 条消息）= 1 + 6 = 7
        assert!(msgs.len() <= 7, "Should have system + 3 turns max");
    }

    #[test]
    fn test_sliding_window_protects_system() {
        let mut msgs = make_context_messages(10);
        sliding_window_compress(&mut msgs, 3);

        assert!(
            msgs.iter().any(|m| matches!(&m.message, ChatMessage::System { .. })),
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

        // 标记第 2 轮的用户消息为 preserved
        msgs[3].preserved = true;
        msgs[3].importance = crate::context::MessageImportance::Important;

        sliding_window_compress(&mut msgs, 2);

        // preserved 消息应该还在
        assert!(
            msgs.iter().any(|m| m.preserved),
            "Preserved message should still be in the list"
        );
    }

    #[test]
    fn test_sliding_window_below_limit() {
        let mut msgs = make_context_messages(2); // 只有 2 轮
        let result = sliding_window_compress(&mut msgs, 5);

        assert!(matches!(result, CompressResult::NotNeeded));
        assert_eq!(msgs.len(), 1 + 2 * 2); // 1 system + 2 turns * 2 = 5
    }

    #[test]
    fn test_sliding_window_exact_limit() {
        let mut msgs = make_context_messages(5);
        let result = sliding_window_compress(&mut msgs, 5);
        assert!(matches!(result, CompressResult::NotNeeded));
    }

    #[test]
    fn test_sliding_window_with_tool_calls() {
        let mut msgs = make_context_messages_with_tools(10);

        let result = sliding_window_compress(&mut msgs, 3);

        assert!(matches!(result, CompressResult::SlidingWindowCompressed { .. }));
        // System(1) + 3轮 * 每条轮消息数(1 User + 1 Assistant_tc + 1 Tool + 1 Assistant = 4) = 1 + 12 = 13
        assert!(msgs.len() <= 13, "Should keep at most 13 messages");
    }

    #[test]
    fn test_sliding_window_with_tools_preserves_mid_turn() {
        let mut msgs = make_context_messages_with_tools(5);

        // 标记第二轮中的 Tool 消息为 preserved
        // 第 2 轮: index 5=User, 6=Assistant_tc, 7=Tool, 8=Assistant
        msgs[7].preserved = true;

        let result = sliding_window_compress(&mut msgs, 2);

        assert!(matches!(result, CompressResult::SlidingWindowCompressed { .. }));

        // preserved tool 消息应该还在
        assert!(msgs.iter().any(|m| m.preserved));
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
        };
        let mut stats = ContextStats::default();

        let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats);

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
            token_limit: 10, // 非常小，触发保底截断
            max_turns: 2,
            trigger_ratio: 0.1,
            enable_async_summary: false,
        };
        let mut stats = ContextStats::default();

        let _result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats);

        assert!(stats.compressed);
    }

    #[test]
    fn test_auto_not_needed() {
        let mut msgs = make_context_messages(2);
        let estimator = TokenEstimator::new();
        let strategy = ContextStrategy::Auto {
            token_limit: 100_000,
            max_turns: 20,
            trigger_ratio: 0.9, // 90% 才触发，远高于当前 token
            enable_async_summary: false,
        };
        let mut stats = ContextStats::default();

        let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats);

        assert!(matches!(result, CompressResult::NotNeeded));
    }

    #[test]
    fn test_hard_truncate_basic() {
        let mut msgs = make_context_messages(3);
        let estimator = TokenEstimator::new();

        // 3 轮对话 = System(1) + User*3 + Assistant*3 = 7 条消息
        // 用极小的 token limit 确保截断
        let result = hard_truncate(&mut msgs, 5, &estimator);

        assert!(matches!(result, CompressResult::HardTruncated { .. }));
        // System 消息必须保留
        assert!(
            msgs.iter().any(|m| matches!(&m.message, ChatMessage::System { .. })),
            "System must be preserved"
        );
    }

    #[test]
    fn test_hard_truncate_not_needed() {
        let mut msgs = make_context_messages(1);
        let estimator = TokenEstimator::new();

        // 很大的 limit，不需要截断
        let result = hard_truncate(&mut msgs, 100_000, &estimator);

        assert!(matches!(result, CompressResult::NotNeeded));
        assert_eq!(msgs.len(), 3); // System + User + Assistant
    }
}
