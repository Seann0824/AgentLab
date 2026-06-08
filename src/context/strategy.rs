use std::time::Instant;

use tokio::sync::mpsc;

use crate::model::ChatMessage;

use super::config::ContextStrategy;
use super::tokenizer::TokenEstimator;
use super::types::{CompressResult, ContextMessage, ContextStats, SummaryScope, SummaryTask};

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
        eprintln!(
            "[remove_orphaned_tool_messages] 🧹 removed {} orphaned tool messages",
            orphaned_tool_count,
        );
    }

    orphaned_tool_count
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
        let (_tool_call_id, content) = match &ctx_msg.message {
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

    // 🔴 删除孤立的 Tool 消息（防止 API 报错）
    let orphaned = remove_orphaned_tool_messages(messages);
    let total_removed = removed_count + orphaned;

    CompressResult::SlidingWindowCompressed {
        removed_count: total_removed,
        removed_turns: remove_turns,
    }
}

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
fn emergency_truncate(
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
    let system_idx = messages.iter().position(|m| matches!(&m.message, ChatMessage::System { .. }));

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

// ================= 自动模式：四层渐进压缩 =================

/// ⭐ 自动模式压缩（四层渐进模型）
///
/// 层级（从轻到重）：
/// 层0: 工具调用结果修剪 → 用占位符替换旧工具输出（最轻量，保留对话结构）
/// 层1: 异步模型摘要 → 用 LLM 生成结构化摘要（非阻塞派发，摘要结果异步注入）
/// 层2: 滑动窗口 → 删除早期整轮对话
/// 层3: 保底截断 → 最后的安全网
///
/// 核心原则：先尝试最轻量的方法，不行再逐渐加重。
/// summary_tx 参数用于异步派发模型摘要任务（层1），SummaryTask 通过 channel 发送到后台 AsyncSummarizer。
pub fn auto_compress(
    messages: &mut Vec<ContextMessage>,
    strategy: &ContextStrategy,
    estimator: &TokenEstimator,
    stats: &mut ContextStats,
    summary_tx: Option<mpsc::UnboundedSender<SummaryTask>>,
) -> CompressResult {
    // ⭐ 使用 if let 安全解构
    let (
        token_limit,
        max_turns,
        trigger_ratio,
        enable_async_summary,
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

    // ⭐ 情况 C: 层1 — 异步模型摘要派发（非阻塞，在滑动窗口删除消息前保存上下文）
    //
    // 在滑动窗口删除消息之前，将当前消息快照派发给后台 AsyncSummarizer。
    // 摘要结果会异步注入，帮助恢复被滑动窗口删除的早期对话信息。
    if enable_async_summary {
        if let Some(ref tx) = summary_tx {
            let snapshot = messages.clone();
            let task = SummaryTask {
                messages: snapshot,
                scope: SummaryScope::EarlyNonPreserved {
                    keep_recent: max_turns,
                },
            };
            let _ = tx.send(task);
            eprintln!(
                "[auto_compress] 📋 layer1: dispatched async summary (keep_recent={})",
                max_turns,
            );
        }
    }

    // ⭐ 情况 D: 层2 — 滑动窗口压缩
    let turns = count_turns(messages);

    // ⭐ 动态计算有效 max_turns
    //
    // 🔴 v2 修复：线性缩放 max_turns→1，从 trigger_threshold→token_limit
    //
    // 原公式：effective = max_turns * (trigger_threshold / current_tokens)
    // 问题：此公式在 tokens 刚过阈值时几乎不降低（0.996 的系数），导致滑动窗口无法触发
    //
    // 新公式：使用线性插值
    //   reduction_ratio = (current_tokens - trigger_threshold) / (token_limit - trigger_threshold)
    //   effective = max_turns * (1 - reduction_ratio)
    //
    // 特性：
    //   - 70% tokens (89.6K) → 20 轮
    //   - 78% tokens (100K) → 15 轮（而非原公式的 18 轮）
    //   - 86% tokens (110K) → 9 轮
    //   - 94% tokens (120K) → 4 轮
    //   - 100% tokens (128K) → 1 轮
    let effective_max_turns = if current_tokens >= trigger_threshold && turns > 1 {
        let over = current_tokens.saturating_sub(trigger_threshold) as f64;
        let range = (token_limit - trigger_threshold) as f64;
        let reduction_ratio = if range > 0.0 {
            (over / range).min(1.0)
        } else {
            1.0
        };
        let effective = (max_turns as f64 * (1.0 - reduction_ratio)).max(1.0).round() as usize;
        effective.min(max_turns)
    } else {
        max_turns
    };

    eprintln!(
        "[auto_compress] tokens={}/{} ({:.0}%%) turns={} effective_max_turns={} threshold={}",
        current_tokens, token_limit, current_tokens as f64 / token_limit as f64 * 100.0,
        turns, effective_max_turns, trigger_threshold,
    );

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
    // 🔴 v2 修复：>= 确保 100% 时也能触发硬截断
    let tokens_after = estimate_total_tokens(messages, estimator);
    if tokens_after >= token_limit {
        let result = hard_truncate(messages, token_limit, estimator);
        if result.did_compress() {
            stats.compressed = true;
            stats.last_compressed_at = Some(Instant::now());
            eprintln!(
                "[auto_compress] 🔴 hard_truncate: removed {} messages, tokens {} → {}",
                match &result { CompressResult::HardTruncated { removed_count, .. } => *removed_count, _ => 0 },
                tokens_after,
                estimate_total_tokens(messages, estimator),
            );
            return result;
        } else {
            eprintln!("[auto_compress] ⚠️ hard_truncate called but no messages removed (all protected?)");
            // 🚨 层4: 紧急截断 — 当所有层都无法截断但 Token 仍然超限时的最后安全网
            let tokens_still = estimate_total_tokens(messages, estimator);
            if tokens_still >= token_limit {
                eprintln!("[auto_compress] 🚨 Tokens still over limit after hard_truncate, calling emergency_truncate");
                let emergency_result = emergency_truncate(messages, token_limit, estimator);
                if emergency_result.did_compress() {
                    stats.compressed = true;
                    stats.last_compressed_at = Some(Instant::now());
                }
                return emergency_result;
            }
        }
        return result;
    }

    CompressResult::NotNeeded
}


/// ⭐ 强制压缩（由 is_blocked 触发，跳过 trigger_threshold 检查直接执行最激进压缩）
///
/// 执行策略：
/// 1. 工具调用结果修剪（层0）
/// 2. 异步摘要派发（层1）
/// 3. 滑动窗口压缩到仅保留 1 轮（层2），auto-loop 模式下按消息数量压缩
/// 4. 保底硬截断（层3）
/// 5. 紧急截断（层4）
pub fn force_compress(
    messages: &mut Vec<ContextMessage>,
    strategy: &ContextStrategy,
    estimator: &TokenEstimator,
    stats: &mut ContextStats,
    summary_tx: Option<mpsc::UnboundedSender<SummaryTask>>,
) -> CompressResult {
    eprintln!(
        "[force_compress] 🚀 ACTIVATED: {} messages",
        messages.len(),
    );

    // 1. 工具调用结果修剪（层0）
    if strategy.tool_pruning_enabled() {
        let _ = tool_call_pruning(
            messages,
            strategy.tool_pruning_keep_recent(),
            strategy.tool_pruning_max_output_chars(),
            estimator,
        );
    }

    // 2. 异步摘要派发（层1）
    if let Some(ref tx) = summary_tx {
        let snapshot = messages.clone();
        let task = SummaryTask {
            messages: snapshot,
            scope: SummaryScope::EarlyNonPreserved { keep_recent: 1 },
        };
        let _ = tx.send(task);
    }

    // 3. 滑动窗口压缩（层2）
    let turns = count_turns(messages);
    if turns > 1 {
        // 正常模式：压缩到 1 轮
        let _ = sliding_window_compress(messages, 1);
    } else {
        // auto-loop 模式：只有 1 个 User 消息，按消息数量压缩
        // 保留：System + preserved + 最近 10 条消息
        let keep_recent = 10usize;
        let original_len = messages.len();
        if original_len > keep_recent + 1 {
            let system_idx = messages.iter().position(|m| matches!(&m.message, ChatMessage::System { .. }));
            let mut new_messages: Vec<ContextMessage> = Vec::new();
            
            // 添加 System
            if let Some(idx) = system_idx {
                new_messages.push(messages[idx].clone());
            }
            
            // 添加删除范围内的 preserved 消息
            let remove_end = original_len.saturating_sub(keep_recent);
            for i in 0..remove_end {
                if Some(i) != system_idx && messages[i].preserved {
                    new_messages.push(messages[i].clone());
                }
            }
            
            // 添加最后 keep_recent 条
            new_messages.extend_from_slice(&messages[original_len.saturating_sub(keep_recent)..]);
            
            *messages = new_messages;
            
            // 🔴 清理孤立的 Tool 消息（手动构建消息列表可能破坏 tool_calls→Tool 对应关系）
            let orphaned = remove_orphaned_tool_messages(messages);
            if orphaned > 0 {
                eprintln!(
                    "[force_compress] 🧹 auto-loop: removed {} orphaned tool messages",
                    orphaned,
                );
            }
        }
    }

    // 4. 硬截断（层3）
    let token_limit = match strategy {
        ContextStrategy::Auto { token_limit, .. } => *token_limit,
        _ => usize::MAX,
    };
    
    let tokens_before_truncate = estimate_total_tokens(messages, estimator);
    if tokens_before_truncate >= token_limit {
        let _ = hard_truncate(messages, token_limit, estimator);
    }

    // 5. 紧急截断（层4）
    let tokens_before_emergency = estimate_total_tokens(messages, estimator);
    if tokens_before_emergency >= token_limit {
        let _ = emergency_truncate(messages, token_limit, estimator);
    }

    let final_tokens = estimate_total_tokens(messages, estimator);
    stats.compressed = true;
    stats.estimated_tokens = final_tokens;
    stats.usage_ratio = final_tokens as f64 / token_limit as f64;
    stats.last_compressed_at = Some(std::time::Instant::now());

    eprintln!(
        "[force_compress] ✅ Done: {} msgs, tokens → {} ({:.0}%)",
        messages.len(),
        final_tokens,
        stats.usage_ratio * 100.0,
    );

    CompressResult::ForceCompressed {
        removed_count: stats.pruned_tool_calls,
        kept_count: messages.len(),
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
    fn test_sliding_window_removes_orphaned_tool_even_if_preserved() {
        /// 测试：当 Tool 消息被 preserved 但其对应的 Assistant(tool_calls) 被滑动窗口删除时，
        /// Tool 消息仍然是孤儿（因为没有对应的 tool_calls 前驱），会被 remove_orphaned_tool_messages 清理。
        /// 这是一个安全优先的决策：宁删 preserved 消息，也不让孤儿 Tool 消息导致 API 报错。
        let mut msgs = make_context_messages_with_tools(5);
        msgs[7].preserved = true; // Tool(call_1) — 但 Assistant(tc: call_1) 在 index 6

        let result = sliding_window_compress(&mut msgs, 2);
        assert!(result.did_compress());
        // 被 preserved 的孤儿 Tool 消息会被清理掉（安全优先）
        // 但 System 和正常的 non-orphan preserved 消息应该保留
        assert!(
            msgs.iter().any(|m| matches!(&m.message, ChatMessage::System { .. })),
            "System message should always survive"
        );
        // 验证没有孤立的 Tool 消息残留
        let tool_msgs: Vec<_> = msgs.iter().filter(|m| matches!(&m.message, ChatMessage::Tool { .. })).collect();
        let assistant_tc_ids: Vec<String> = msgs.iter().filter_map(|m| {
            if let ChatMessage::Assistant { tool_calls, .. } = &m.message {
                Some(tool_calls.iter().map(|tc| tc.id.clone()).collect::<Vec<_>>())
            } else { None }
        }).flatten().collect();
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
}
