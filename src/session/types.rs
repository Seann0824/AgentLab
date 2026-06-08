use serde::{Deserialize, Serialize};

use crate::context::ContextStrategy;
use crate::model::{ChatMessage, ToolCall};

/// ⭐ 可序列化的消息类型（用于持久化）
///
/// ChatMessage 包含工具调用等复杂结构，直接序列化可能丢失信息。
/// 这里定义一个显式的、自描述的格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum SerializableMessage {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant {
        content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<SerializableToolCall>,
    },
    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// 可序列化的工具调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl From<&ChatMessage> for SerializableMessage {
    fn from(msg: &ChatMessage) -> Self {
        match msg {
            ChatMessage::System { content } => SerializableMessage::System {
                content: content.clone(),
            },
            ChatMessage::User { content } => SerializableMessage::User {
                content: content.clone(),
            },
            ChatMessage::Assistant {
                content,
                tool_calls,
            } => SerializableMessage::Assistant {
                content: content.clone(),
                tool_calls: tool_calls
                    .iter()
                    .map(|tc| SerializableToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    })
                    .collect(),
            },
            ChatMessage::Tool {
                tool_call_id,
                content,
            } => SerializableMessage::Tool {
                tool_call_id: tool_call_id.clone(),
                content: content.clone(),
            },
        }
    }
}

impl From<SerializableMessage> for ChatMessage {
    fn from(msg: SerializableMessage) -> Self {
        match msg {
            SerializableMessage::System { content } => ChatMessage::system(content),
            SerializableMessage::User { content } => ChatMessage::user(content),
            SerializableMessage::Assistant {
                content,
                tool_calls,
            } => {
                let tcs: Vec<ToolCall> = tool_calls
                    .into_iter()
                    .map(|tc| ToolCall {
                        id: tc.id,
                        name: tc.name,
                        arguments: tc.arguments,
                    })
                    .collect();
                if tcs.is_empty() {
                    ChatMessage::assistant(content)
                } else {
                    ChatMessage::assistant_tool_calls(content, tcs)
                }
            }
            SerializableMessage::Tool {
                tool_call_id,
                content,
            } => ChatMessage::tool(tool_call_id, content),
        }
    }
}

/// ⭐ 会话数据（完整的可持久化状态）
///
/// 包含：
/// - 元数据（名称、创建/更新时间）
/// - 消息列表（所有对话消息 + preserved 标记）
/// - 上下文策略（用于重建 ContextManager）
/// - 额外的元信息（如当前工作目录等）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// 会话名称
    pub name: String,
    /// 创建时间
    pub created_at: String,
    /// 最后修改时间
    pub updated_at: String,
    /// 消息列表（带 preserved 标记）
    pub messages: Vec<SerializedContextMessage>,
    /// 压缩策略
    pub strategy: ContextStrategy,
    /// 当前工作目录（用于恢复上下文）
    pub current_dir: String,
    /// 版本（便于未来升级迁移）
    pub version: u32,
}

/// 带 preserved 标记的可序列化消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedContextMessage {
    /// 消息内容
    pub message: SerializableMessage,
    /// 是否标记为永久保留
    #[serde(default)]
    pub preserved: bool,
}
