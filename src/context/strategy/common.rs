use crate::context::{ContextMessage, TokenEstimator};
use crate::model::ChatMessage;

/// ⭐ 对话轮次计数器（统一逻辑）
///
/// 一轮的定义：以 User 消息为起始，到下一个 User 消息之前（或末尾）结束。
/// System 消息不计入轮次。
///
/// 示例：
///   System, User1, Assistant1(tc), Tool1, Assistant1(ok), User2, Assistant2
///   → 2 turns (User1 + User2)
pub(super) fn count_turns(messages: &[ContextMessage]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(&m.message, ChatMessage::User { .. }))
        .count()
}

/// ⭐ 找到每轮对话的消息范围
///
/// 返回每个 User 消息的索引位置，这些就是每轮的起始边界。
pub(super) fn find_turn_boundaries(messages: &[ContextMessage]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(&m.message, ChatMessage::User { .. }))
        .map(|(i, _)| i)
        .collect()
}

/// ⭐ 删除孤立的 Tool 消息（没有对应的 Assistant tool_calls 前驱）
///
/// 当压缩操作删除 Assistant(tool_calls) 但保留了对应的 Tool 响应时，
/// 会产生孤儿 Tool 消息。这些消息会导致 OpenAI API 报错：
/// "Messages with role 'tool' must be a response to a preceding message with 'tool_calls'"
///
/// 策略：从后往前遍历，收集所有"活跃"的 tool_call_id（来自 Assistant tool_calls），
/// 然后删除 Tool 消息中不属于任何活跃 tool_call_id 的。
/// 返回删除的孤儿消息数量。
pub fn remove_orphaned_tool_messages(messages: &mut Vec<ContextMessage>) -> usize {
    let mut active_tool_call_ids: Vec<String> = Vec::new();
    let mut orphaned_tool_count = 0;

    // 从后往前扫描，先收集所有 Assistant tool_calls 中的 tool_call_id
    for i in (0..messages.len()).rev() {
        if let ChatMessage::Assistant { tool_calls, .. } = &messages[i].message {
            for tc in tool_calls {
                if !active_tool_call_ids.contains(&tc.id) {
                    active_tool_call_ids.push(tc.id.clone());
                }
            }
        }
    }

    // 再次从后往前遍历，移除孤立的 Tool 消息
    let mut i = messages.len();
    while i > 0 {
        i -= 1;
        if let ChatMessage::Tool { tool_call_id, .. } = &messages[i].message {
            if !active_tool_call_ids.contains(tool_call_id) {
                messages.remove(i);
                orphaned_tool_count += 1;
            }
        }
    }

    if orphaned_tool_count > 0 {
        crate::debug!(
            "[remove_orphaned_tool_messages] 🧹 removed {} orphaned tool messages",
            orphaned_tool_count,
        );
    }

    orphaned_tool_count
}

/// 计算消息列表的总 token 数
pub(super) fn estimate_total_tokens(
    messages: &[ContextMessage],
    estimator: &TokenEstimator,
) -> usize {
    let raw: Vec<ChatMessage> = messages.iter().map(|m| m.message.clone()).collect();
    estimator.estimate_messages(&raw)
}
