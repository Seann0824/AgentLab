use openai_api_rs::v1::chat_completion::Content;
use serde::{Deserialize, Serialize};

use crate::base::message::Message;

/// 面向前端的消息结构。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<Vec<ToolCallInfo>>,
    pub metadata: Option<serde_json::Value>,
}

/// 工具调用描述信息。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 会话列表项。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
}

impl ChatMessage {
    /// 把内部 `Message` 转换为面向前端的 `ChatMessage`。
    /// 只处理 `Content::Text`；其他 content 类型返回空字符串。
    pub fn from_message(msg: &Message) -> Self {
        let content = match &msg.naive_message.content {
            Content::Text(text) => text.clone(),
            _ => String::new(),
        };

        let role = match msg.naive_message.role {
            openai_api_rs::v1::chat_completion::MessageRole::user => "user",
            openai_api_rs::v1::chat_completion::MessageRole::assistant => "assistant",
            openai_api_rs::v1::chat_completion::MessageRole::tool => "tool",
            openai_api_rs::v1::chat_completion::MessageRole::system => "system",
            openai_api_rs::v1::chat_completion::MessageRole::function => "tool",
        }
        .to_string();

        let tool_calls = msg.naive_message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|tc| ToolCallInfo {
                    id: tc.id.clone(),
                    name: tc.function.name.clone().unwrap_or_default(),
                    arguments: tc.function.arguments.clone().unwrap_or_default(),
                })
                .collect()
        });

        Self {
            id: msg.id.clone(),
            role,
            content,
            timestamp: msg.timestamp.timestamp(),
            tool_call_id: msg.naive_message.tool_call_id.clone(),
            tool_calls,
            metadata: msg.metadata.clone(),
        }
    }
}
