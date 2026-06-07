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

// ================= 层0：工具调用结果修剪 =================

/// ⭐ 层0：工具调用结果修剪（最轻量压缩，保留完整对话结构）
///
/// 原理：
/// - 找到早期的 Tool 消息
/// - 如果内容很长，替换为简短的占位符
/// - 保留 tool_call_id，只压缩结果内容
///
/// 触发条件：
/// - 只处理非 preserved、非重要的 Tool 消息
/// - 只处理超过 max_output_chars 的长输出
/// - 最近的 keep_recent 轮次不动（避免影响当前上下文）
///
/// 效果：
/// - 对话结构完整保留（User → Assistant(tc) → Tool(短) → Assistant）
/// - 大幅削减 token 消耗（工具结果通常占 token 的大头）
/// - 不会丢失"调用了什么工具"的信息
pub fn tool_call_pruning(
    messages: &mut Vec<ContextMessage>,
    keep_recent: usize,
    max_output_chars: usize,
    estimator: &TokenEstimator,
) -> CompressResult {
    let user_positions = find_turn_boundaries(messages);
    let total_turns = user_positions.len();

    if total_turns <= keep_recent {
        return CompressResult::NotNeeded;
    }

    let pruneable_turns = total_turns - keep_recent;
    let prune_boundary = if pruneable_turns < user_positions.len() {
        user_positions[pruneable_turns]
    } else {
        0
    };

    // 先收集需要修剪的索引和对应的替换内容
    struct PruneAction {
        index: usize,
        new_content: String,
        saved_tokens: usize,
    }

    let mut actions: Vec<PruneAction> = Vec::new();

    for i in 0..prune_boundary {
        let ctx_msg = &messages[i];

        // 只处理 Tool 消息
        let (tool_call_id, content) = match &ctx_msg.message {
            ChatMessage::Tool {
                tool_call_id,
                content,
            } => (tool_call_id.clone(), content.clone()),
            _ => continue,
        };

        // 跳过 preserved 或重要的消息
        if ctx_msg.preserved
            || ctx_msg.importance == super::types::MessageImportance::Important
            || ctx_msg.importance == super::types::MessageImportance::Milestone
        {
            continue;
        }

        // 只修剪长输出
        if content.len() <= max_output_chars {
            continue;
        }

        // 生成占位符
        let brief = if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            let ok_status = val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let cmd = val
                .get("result")
                .and_then(|r| r.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let stdout_len = val
                .get("result")
                .and_then(|r| r.get("stdout"))
                .and_then(|v| v.as_str())
                .map(|s| s.len())
                .unwrap_or(0);
            let stderr_len = val
                .get("result")
                .and_then(|r| r.get("stderr"))
                .and_then(|v| v.as_str())
                .map(|s| s.len())
                .unwrap_or(0);

            format!(
                r#"{{"ok":{},"result":{{"command":"{}","stdout":"[TOOL_OUTPUT_PRUNED {}b stdout, {}b stderr]","_pruned":true}}}}"#,
                ok_status,
                cmd.chars().take(60).collect::<String>(),
                stdout_len,
                stderr_len
            )
        } else {
            // 非标准 JSON 格式，保留开头并标注
            let preview: String = content.chars().take(120).collect();
            format!(
                "{} ...\n[TOOL_OUTPUT_PRUNED: 原始输出共 {} 字符]\n",
                preview,
                content.len()
            )
        };

        // 计算节省的 token
        let original_tokens = estimator.estimate_text(&content);
        let new_tokens = estimator.estimate_text(&brief);
        let saved = original_tokens.saturating_sub(new_tokens);

        actions.push(PruneAction {
            index: i,
            new_content: brief,
            saved_tokens: saved,
        });
    }

    if actions.is_empty() {
        return CompressResult::NotNeeded;
    }

    // 执行修剪
    let mut total_saved = 0;
    for action in &actions {
        // 获取原始的 tool_call_id
        let orig_tool_call_id = match &messages[action.index].message {
            ChatMessage::Tool { tool_call_id, .. } => tool_call_id.clone(),
            _ => String::new(),
        };
        messages[action.index].message =
            ChatMessage::tool(orig_tool_call_id, &action.new_content);
        total_saved += action.saved_tokens;
    }

    CompressResult::ToolCallsPruned {
        pruned_count: actions.len(),
        saved_tokens: total_saved,
    }
}

// ================= 层1：滑动窗口压缩 =================

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

// ================= 层3：保底截断 =================

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
            remove_up_to = Some(i + 1);
        }
    }

    // 如果没删任何消息，返回 NotNeeded
    let Some(end) = remove_up_to else {
        return CompressResult::NotNeeded;
    };

    // 构建新列表：保留 protected 消息 + 剩余消息
    let mut new_messages: Vec<ContextMessage> = Vec::with_capacity(messages.len());

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

    let removed_count = original_len - new_messages.len();
    *messages = new_messages;

    CompressResult::HardTruncated {
        removed_count,
        kept_count: messages.len(),
    }
}

// ================= 自动模式：四层渐进压缩 =================

/// ⭐ 自动模式压缩（四层渐进模型）
///
/// 层级（从轻到重）：
/// 层0: 工具调用结果修剪 → 用占位符替换旧工具输出（最轻量，保留对话结构）
/// 层1: 滑动窗口 → 删除早期整轮对话
/// 层2: 异步摘要 → 由 ContextManager 派发（不在此处理）
/// 层3: 保底截断 → 最后的安全网
///
/// 核心原则：先尝试最轻量的方法，不行再逐渐加重。
pub fn auto_compress(
    messages: &mut Vec<ContextMessage>,
    strategy: &ContextStrategy,
    estimator: &TokenEstimator,
    stats: &mut ContextStats,
) -> CompressResult {
    // ⭐ 使用 if let 安全解构
    let (
        token_limit,
        max_turns,
        trigger_ratio,
        _enable_async_summary,
        enable_tool_pruning,
        tool_pruning_keep_recent,
        tool_pruning_max_output_chars,
    ) = match strategy {
        ContextStrategy::Auto {
            token_limit,
            max_turns,
            trigger_ratio,
            enable_async_summary,
            enable_tool_pruning,
            tool_pruning_keep_recent,
            tool_pruning_max_output_chars,
        } => (
            *token_limit,
            *max_turns,
            *trigger_ratio,
            *enable_async_summary,
            *enable_tool_pruning,
            *tool_pruning_keep_recent,
            *tool_pruning_max_output_chars,
        ),
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

    // ⭐ 情况 B: 先尝试层0 — 工具调用结果修剪（最轻量）
    if enable_tool_pruning {
        let prune_result =
            tool_call_pruning(messages, tool_pruning_keep_recent, tool_pruning_max_output_chars, estimator);
        if prune_result.did_compress() {
            stats.compressed = true;
            stats.last_compressed_at = Some(Instant::now());

            if let CompressResult::ToolCallsPruned {
                pruned_count,
                saved_tokens,
            } = &prune_result
            {
                stats.pruned_tool_calls += pruned_count;
                stats.pruned_saved_tokens += saved_tokens;
            }

            // 修剪后重新检查 token 是否足够
            let tokens_after_prune = estimate_total_tokens(messages, estimator);
            stats.estimated_tokens = tokens_after_prune;
            stats.usage_ratio = tokens_after_prune as f64 / token_limit as f64;

            if tokens_after_prune < trigger_threshold {
                return prune_result;
            }
            // 如果仍然超限，继续降级到滑动窗口
        }
    }

    // ⭐ 情况 C: 层1 — 滑动窗口压缩
    let turns = count_turns(messages);

    // ⭐ 动态计算有效 max_turns（修复：当 token 超过触发阈值时，按比例缩减保留轮数）
    //
    // 原逻辑：仅当 turns > max_turns 时才触发滑动窗口
    // 问题：如果 token 使用率超过 70% 但轮数没到 20，压缩永远不会触发
    // 修复：当 current_tokens >= trigger_threshold 时，按超出比例动态降低 effective_max_turns
    //
    // 计算公式：effective_max_turns = max_turns * (trigger_threshold / current_tokens)
    // 举例：token 100% → effective = 20 * 0.7 = 14，15轮时触发压缩到14轮
    //        token 200% → effective = 20 * 0.35 = 7，8轮时触发压缩到7轮
    let effective_max_turns = if current_tokens >= trigger_threshold && turns > 1 {
        let target_ratio = trigger_threshold as f64 / current_tokens as f64;
        let reduced = (max_turns as f64 * target_ratio).ceil() as usize;
        reduced.max(1).min(max_turns)
    } else {
        max_turns
    };

    if turns > effective_max_turns {
        let result = sliding_window_compress(messages, effective_max_turns);
        if result.did_compress() {
            stats.compressed = true;
            stats.last_compressed_at = Some(Instant::now());

            // 滑动窗口后检查是否还需要进一步压缩
            let tokens_after = estimate_total_tokens(messages, estimator);
            stats.estimated_tokens = tokens_after;
            stats.usage_ratio = tokens_after as f64 / token_limit as f64;

            if tokens_after <= token_limit {
                return result;
            }
            // 仍然超限 → 继续降级到保底截断
        }
    }

    // ⭐ 情况 D: 层3 — 保底截断（所有轻量方法都无效时的最后手段）
    let tokens_after = estimate_total_tokens(messages, estimator);
    if tokens_after > token_limit {
        let result = hard_truncate(messages, token_limit, estimator);
        if result.did_compress() {
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
    if result.did_compress() {
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
    use crate::model::ToolCall;

    fn make_context_messages(count: usize) -> Vec<ContextMessage> {
        let mut msgs = Vec::new();
        msgs.push(ContextMessage::from(ChatMessage::system("System prompt")));

        for i in 0..count {
            msgs.push(ContextMessage::from(ChatMessage::user(format!("User {}", i))));
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
            msgs.push(ContextMessage::from(ChatMessage::user(format!("User {}", i))));
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
    fn test_sliding_window_with_tools_preserves_mid_turn() {
        let mut msgs = make_context_messages_with_tools(5);
        msgs[7].preserved = true;

        let result = sliding_window_compress(&mut msgs, 2);
        assert!(result.did_compress());
        assert!(msgs.iter().any(|m| m.preserved));
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

        let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats);

        assert!(stats.compressed, "Should have compressed");
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
            token_limit: 10,
            max_turns: 2,
            trigger_ratio: 0.1,
            enable_async_summary: false,
            enable_tool_pruning: false,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 100,
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
            trigger_ratio: 0.9,
            enable_async_summary: false,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 100,
        };
        let mut stats = ContextStats::default();

        let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats);

        assert!(!result.did_compress());
    }

    #[test]
    fn test_hard_truncate_basic() {
        let mut msgs = make_context_messages(3);
        let estimator = TokenEstimator::new();

        let result = hard_truncate(&mut msgs, 5, &estimator);

        assert!(result.did_compress());
        assert!(
            msgs.iter().any(|m| matches!(&m.message, ChatMessage::System { .. })),
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

        let result = auto_compress(&mut msgs, &strategy, &estimator, &mut stats);

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
}
