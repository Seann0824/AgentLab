use std::time::Instant;

use tokio::sync::mpsc;

use crate::context::{
    CompressResult, ContextMessage, ContextStats, ContextStrategy, SummaryScope, SummaryTask,
    TokenEstimator,
};
use crate::model::ChatMessage;

use super::common::{count_turns, estimate_total_tokens, remove_orphaned_tool_messages};
use super::pruning::tool_call_pruning;
use super::sliding::sliding_window_compress;
use super::truncate::{emergency_truncate, hard_truncate};

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
            crate::debug!(
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
        let prune_result = tool_call_pruning(
            messages,
            tool_pruning_keep_recent,
            tool_pruning_max_output_chars,
            estimator,
        );
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
            crate::debug!(
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
        let effective = (max_turns as f64 * (1.0 - reduction_ratio))
            .max(1.0)
            .round() as usize;
        effective.min(max_turns)
    } else {
        max_turns
    };

    crate::debug!(
        "[auto_compress] tokens={}/{} ({:.0}%%) turns={} effective_max_turns={} threshold={}",
        current_tokens,
        token_limit,
        current_tokens as f64 / token_limit as f64 * 100.0,
        turns,
        effective_max_turns,
        trigger_threshold,
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
            crate::debug!(
                "[auto_compress] 🔴 hard_truncate: removed {} messages, tokens {} → {}",
                match &result {
                    CompressResult::HardTruncated { removed_count, .. } => *removed_count,
                    _ => 0,
                },
                tokens_after,
                estimate_total_tokens(messages, estimator),
            );
            return result;
        } else {
            crate::debug!(
                "[auto_compress] ⚠️ hard_truncate called but no messages removed (all protected?)"
            );
            // 🚨 层4: 紧急截断 — 当所有层都无法截断但 Token 仍然超限时的最后安全网
            let tokens_still = estimate_total_tokens(messages, estimator);
            if tokens_still >= token_limit {
                crate::debug!(
                    "[auto_compress] 🚨 Tokens still over limit after hard_truncate, calling emergency_truncate"
                );
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
    crate::debug!("[force_compress] 🚀 ACTIVATED: {} messages", messages.len(),);

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
            let system_idx = messages
                .iter()
                .position(|m| matches!(&m.message, ChatMessage::System { .. }));
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
                crate::debug!(
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

    crate::debug!(
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
