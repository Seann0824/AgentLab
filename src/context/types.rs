use std::time::Instant;

use crate::model::ChatMessage;

/// 工具调用修剪记录（用于调试和统计）
#[derive(Debug, Clone)]
pub struct PrunedToolCall {
    /// 工具名称
    pub tool_name: String,
    /// 原始工具输出的长度（字符数）
    pub original_len: usize,
    /// 修剪后占位符的长度（字符数）
    pub placeholder_len: usize,
    /// 节省的 token 估算
    pub saved_tokens: usize,
}

/// 消息重要性标签
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageImportance {
    /// 普通对话消息
    Normal,
    /// 关键上下文（如文件读取结果、项目结构发现）
    Important,
    /// 里程碑决策（如选择了某个技术方案）
    Milestone,
}

/// 扩展的消息类型，增加持久化标记和重要性
#[derive(Debug, Clone)]
pub struct ContextMessage {
    /// 原始 ChatMessage
    pub message: ChatMessage,
    /// 是否标记为"永久保留"
    /// 标记后不会被滑动窗口丢弃，仅在所有策略都无效时由摘要处理
    pub preserved: bool,
    /// 消息的重要性标签（辅助摘要决策）
    pub importance: MessageImportance,
}

impl ContextMessage {
    /// 标记为永久保留
    pub fn preserve(mut self) -> Self {
        self.preserved = true;
        self.importance = MessageImportance::Important;
        self
    }

    /// 自动判断重要性
    pub fn auto_classify(msg: &ChatMessage) -> MessageImportance {
        match msg {
            ChatMessage::Tool { content, .. } => {
                // 文件读取、目录列表等包含结构性信息的结果，标记为重要
                let Ok(val) = serde_json::from_str::<serde_json::Value>(content) else {
                    return MessageImportance::Normal;
                };
                let Some(stdout) = val
                    .get("result")
                    .and_then(|r| r.get("stdout"))
                    .and_then(|s| s.as_str())
                else {
                    return MessageImportance::Normal;
                };
                if is_stdout_structural(stdout) {
                    MessageImportance::Important
                } else {
                    MessageImportance::Normal
                }
            }
            ChatMessage::Assistant { content, .. } => {
                // 包含重要决策标记的回复
                if content.contains("【决策】") || content.contains("方案选择") {
                    MessageImportance::Milestone
                } else {
                    MessageImportance::Normal
                }
            }
            _ => MessageImportance::Normal,
        }
    }
}

impl From<ChatMessage> for ContextMessage {
    fn from(message: ChatMessage) -> Self {
        let importance = ContextMessage::auto_classify(&message);
        ContextMessage {
            message,
            preserved: false,
            importance,
        }
    }
}

/// 上下文统计信息
#[derive(Debug, Clone)]
pub struct ContextStats {
    /// 当前估算 Token 数
    pub estimated_tokens: usize,
    /// 消息总数
    pub message_count: usize,
    /// 对话轮数
    pub turn_count: usize,
    /// 是否已触发压缩
    pub compressed: bool,
    /// 最后压缩时间
    pub last_compressed_at: Option<Instant>,
    /// Token 使用率 (%)
    pub usage_ratio: f64,
    /// 被保留的重要消息数
    pub preserved_count: usize,
    /// ⭐ 工具调用修剪统计
    pub pruned_tool_calls: usize,
    pub pruned_saved_tokens: usize,
}

impl Default for ContextStats {
    fn default() -> Self {
        Self {
            estimated_tokens: 0,
            message_count: 0,
            turn_count: 0,
            compressed: false,
            last_compressed_at: None,
            usage_ratio: 0.0,
            preserved_count: 0,
            pruned_tool_calls: 0,
            pruned_saved_tokens: 0,
        }
    }
}

/// 压缩结果
#[derive(Debug, Clone)]
pub enum CompressResult {
    /// 无需压缩
    NotNeeded,
    /// ⭐ 工具调用结果已用占位符替换（最轻量，保留对话结构）
    ToolCallsPruned {
        pruned_count: usize,
        saved_tokens: usize,
    },
    /// 已通过滑动窗口压缩
    SlidingWindowCompressed {
        removed_count: usize,
        removed_turns: usize,
    },
    /// 已触发异步摘要任务（摘要完成后会自动注入）
    AsyncSummaryDispatched {
        task_id: u64,
    },
    /// 保底截断（所有方法都无效时的最后手段）
    HardTruncated {
        removed_count: usize,
        kept_count: usize,
    },
    /// 🚨 紧急截断（所有压缩层都无效时的最后安全网）
    /// 忽略 preserved 标记（System 除外），仅保留 System + 最后 2 轮对话
    EmergencyTruncated {
        removed_count: usize,
        kept_count: usize,
    },
}

impl CompressResult {
    /// 是否实际发生了压缩
    pub fn did_compress(&self) -> bool {
        !matches!(self, CompressResult::NotNeeded)
    }

    /// 简要描述
    pub fn description(&self) -> &'static str {
        match self {
            CompressResult::NotNeeded => "无需压缩",
            CompressResult::ToolCallsPruned { .. } => "工具调用结果修剪",
            CompressResult::SlidingWindowCompressed { .. } => "滑动窗口压缩",
            CompressResult::AsyncSummaryDispatched { .. } => "异步摘要已派发",
            CompressResult::HardTruncated { .. } => "保底截断",
            CompressResult::EmergencyTruncated { .. } => "🚨 紧急截断",
        }
    }
}

/// 异步摘要任务
#[derive(Debug, Clone)]
pub struct SummaryTask {
    pub messages: Vec<ContextMessage>,
    pub scope: SummaryScope,
}

/// 摘要范围
#[derive(Debug, Clone)]
pub enum SummaryScope {
    /// 摘要所有非 preserved 的早期消息，保留最近 N 轮
    EarlyNonPreserved { keep_recent: usize },
    /// 摘要所有非 preserved 消息
    AllNonPreserved,
}

/// 摘要结果（通过 channel 回传）
#[derive(Debug, Clone)]
pub struct SummaryResult {
    /// 生成的摘要消息（已标记 preserved）
    pub summary_message: ContextMessage,
    /// 被摘要的原始消息范围描述
    pub scope_description: String,
    /// ⭐ 被摘要的原始消息数量（注入时需要删除对应数量的消息）
    pub summarized_count: usize,
}

/// 判断工具输出的 stdout 是否包含结构性上下文信息
/// （文件路径、项目结构、目录列表等）
///
/// 用于自动判定消息重要性、或运行时标记 preserve。
pub fn is_stdout_structural(stdout: &str) -> bool {
    stdout.contains('─')
        || stdout.contains(".rs")
        || stdout.contains("Cargo.toml")
        || stdout.lines().count() > 5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToolCall;

    #[test]
    fn test_auto_classify_tool_with_file_content() {
        let msg = ChatMessage::tool(
            "call_1",
            r#"{"ok": true, "result": {"stdout": "src/main.rs\nsrc/lib.rs\nCargo.toml\n"}}"#,
        );
        assert_eq!(
            ContextMessage::auto_classify(&msg),
            MessageImportance::Important
        );
    }

    #[test]
    fn test_auto_classify_tool_simple() {
        let msg = ChatMessage::tool(
            "call_2",
            r#"{"ok": true, "result": {"stdout": "ok\n"}}"#,
        );
        assert_eq!(
            ContextMessage::auto_classify(&msg),
            MessageImportance::Normal
        );
    }

    #[test]
    fn test_auto_classify_assistant_with_decision() {
        let msg = ChatMessage::assistant("经过分析，【决策】使用 Tokio 作为异步运行时。");
        assert_eq!(
            ContextMessage::auto_classify(&msg),
            MessageImportance::Milestone
        );
    }

    #[test]
    fn test_context_message_preserve() {
        let msg = ChatMessage::user("hello");
        let ctx_msg = ContextMessage::from(msg).preserve();
        assert!(ctx_msg.preserved);
        assert_eq!(ctx_msg.importance, MessageImportance::Important);
    }

    #[test]
    fn test_context_message_from_chat() {
        let msg = ChatMessage::user("hello");
        let ctx_msg = ContextMessage::from(msg);
        assert!(!ctx_msg.preserved);
        assert_eq!(ctx_msg.importance, MessageImportance::Normal);
    }

    #[test]
    fn test_compress_result_did_compress() {
        assert!(!CompressResult::NotNeeded.did_compress());
        assert!(CompressResult::ToolCallsPruned { pruned_count: 1, saved_tokens: 100 }.did_compress());
        assert!(CompressResult::SlidingWindowCompressed { removed_count: 5, removed_turns: 2 }.did_compress());
        assert!(CompressResult::HardTruncated { removed_count: 3, kept_count: 10 }.did_compress());
        assert!(CompressResult::EmergencyTruncated { removed_count: 5, kept_count: 2 }.did_compress());
    }

    #[test]
    fn test_compress_result_description() {
        assert_eq!(CompressResult::NotNeeded.description(), "无需压缩");
        assert_eq!(CompressResult::ToolCallsPruned { pruned_count: 1, saved_tokens: 100 }.description(), "工具调用结果修剪");
        assert_eq!(CompressResult::EmergencyTruncated { removed_count: 5, kept_count: 2 }.description(), "🚨 紧急截断");
    }
}
