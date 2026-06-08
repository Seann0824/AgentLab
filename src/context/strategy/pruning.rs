use crate::context::{CompressResult, ContextMessage, MessageImportance, TokenEstimator};
use crate::model::ChatMessage;

use super::common::find_turn_boundaries;

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
            || ctx_msg.importance == MessageImportance::Important
            || ctx_msg.importance == MessageImportance::Milestone
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
        messages[action.index].message = ChatMessage::tool(orig_tool_call_id, &action.new_content);
        total_saved += action.saved_tokens;
    }

    CompressResult::ToolCallsPruned {
        pruned_count: actions.len(),
        saved_tokens: total_saved,
    }
}
