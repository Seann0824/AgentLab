use crate::context::{CompressResult, ContextMessage, TokenEstimator};
use crate::model::ChatMessage;

use super::common::{estimate_total_tokens, remove_orphaned_tool_messages};

// ================= 层3：保底截断 =================

/// 🚨 层4：紧急截断（最后的安全网中的最后安全网）
///
/// 触发条件：hard_truncate 无法截断（例如所有消息都是 protected）但 Token 仍然超限。
///
/// 策略：
/// - 仅保留 System 消息 + 最后 2 轮对话
/// - 忽略 preserved 标记（System 消息除外）
/// - 如果仍然超限，继续删除最早的非 System 消息直到满足条件
///
/// 这是一个非常激进的截断操作，只在所有其他方法都失败后调用。
pub(super) fn emergency_truncate(
    messages: &mut Vec<ContextMessage>,
    token_limit: usize,
    estimator: &TokenEstimator,
) -> CompressResult {
    let original_len = messages.len();
    let original_tokens = estimate_total_tokens(messages, estimator);

    eprintln!(
        "[emergency_truncate] 🚨 ACTIVATED: {} messages, {} tokens (limit={})",
        original_len, original_tokens, token_limit,
    );

    // 1. 找到 System 消息索引
    let system_idx = messages
        .iter()
        .position(|m| matches!(&m.message, ChatMessage::System { .. }));

    // 2. 找到 User 消息的位置（用于确定轮次边界）
    let user_positions: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(&m.message, ChatMessage::User { .. }))
        .map(|(i, _)| i)
        .collect();

    // 3. 构建新消息列表：System + 最后 2 轮
    let mut new_messages: Vec<ContextMessage> = Vec::new();

    // 先添加 System 消息
    if let Some(idx) = system_idx {
        new_messages.push(messages[idx].clone());
    }

    // 计算保留起点：最后 2 轮对话的起始位置
    let keep_from = if user_positions.len() > 2 {
        // 保留从倒数第 2 轮 User 开始的所有消息
        user_positions[user_positions.len() - 2]
    } else if user_positions.len() > 0 {
        // 如果只有 1 或 2 轮，全部保留
        0
    } else {
        // 没有 User 消息，只保留 System
        messages.len()
    };

    // 添加最后 2 轮的消息（跳过 System，因为已添加）
    for i in keep_from..messages.len() {
        if Some(i) == system_idx {
            continue;
        }
        new_messages.push(messages[i].clone());
    }

    let _after_keep_removed = original_len - new_messages.len();
    *messages = new_messages;

    // 4. 检查 token 是否降到 limit 以下，如果没有则继续删除
    let mut current_tokens = estimate_total_tokens(messages, estimator);
    let mut _additional_removed = 0;

    while current_tokens > token_limit && messages.len() > 1 {
        // 找到第一条非 System 消息并删除
        let remove_idx = messages
            .iter()
            .position(|m| !matches!(&m.message, ChatMessage::System { .. }));
        match remove_idx {
            Some(idx) => {
                messages.remove(idx);
                _additional_removed += 1;
            }
            None => break, // 只剩 System 消息，不能再删了
        }
        current_tokens = estimate_total_tokens(messages, estimator);
    }

    let total_removed = original_len - messages.len();

    // 🔴 删除孤立的 Tool 消息（防止 API 报错）
    let orphaned = remove_orphaned_tool_messages(messages);
    let final_removed = total_removed + orphaned;

    eprintln!(
        "[emergency_truncate] 🚨 FORCED: removed {} messages (keep={}), tokens {} → {}",
        final_removed,
        messages.len(),
        original_tokens,
        current_tokens,
    );

    CompressResult::EmergencyTruncated {
        removed_count: final_removed,
        kept_count: messages.len(),
    }
}

/// ⭐ 层3：保底截断（最后的安全网）
///
/// 触发条件：滑动窗口执行后 Token 仍然超过硬限制
/// 从最早的非保护消息开始截断，直到 Token 低于安全线
///
/// ⭐ 优化：预计算所有消息的 token 数，避免 O(n²) 循环中反复全量估算
pub(super) fn hard_truncate(
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
    // 🔴 v2 修复：使用 < 而非 <=，确保 100% 时也能触发截断
    if total_tokens < token_limit {
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

    // 🔴 删除孤立的 Tool 消息（防止 API 报错）
    let orphaned_tool_count = remove_orphaned_tool_messages(messages);
    let final_removed = removed_count + orphaned_tool_count;

    eprintln!(
        "[hard_truncate] removed {} messages (including {} orphaned tool results), tokens reduced",
        final_removed, orphaned_tool_count,
    );

    CompressResult::HardTruncated {
        removed_count: final_removed,
        kept_count: messages.len(),
    }
}
