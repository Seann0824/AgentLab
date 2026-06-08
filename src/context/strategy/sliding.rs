use std::time::Instant;

use crate::context::{CompressResult, ContextMessage, ContextStats};
use crate::model::ChatMessage;

use super::common::{count_turns, find_turn_boundaries, remove_orphaned_tool_messages};

// ================= 层1：滑动窗口压缩 =================

/// ⭐ 层1：滑动窗口压缩（同步，永不失败）
///
/// 保留规则（按优先级）：
/// 1. 系统提示词 → 始终保留
/// 2. preserved = true 的消息 → 永久保留
/// 3. 最近 N 轮对话 → 保留
///
/// 实现方式：构建新列表，而不是原地删除，避免索引管理问题。
pub(super) fn sliding_window_compress(
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

    // 🔴 删除孤立的 Tool 消息（防止 API 报错）
    let orphaned = remove_orphaned_tool_messages(messages);
    let total_removed = removed_count + orphaned;

    CompressResult::SlidingWindowCompressed {
        removed_count: total_removed,
        removed_turns: remove_turns,
    }
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
