use chrono::Local;
use openai_api_rs::v1::chat_completion::{
    ChatCompletionMessage, Content, MessageRole, ToolCall, ToolCallFunction,
};
use serde::{Deserialize, Serialize};

/// 面向前端的消息结构，也是应用层 canonical 数据格式。
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
    /// 从 `ChatCompletionMessage` 转换为应用层 `ChatMessage`。
    /// 调用方需要提供业务属性 `id` 和 `timestamp`。
    pub fn from_chat_completion_message(
        msg: &ChatCompletionMessage,
        id: String,
        timestamp: i64,
    ) -> Self {
        let content = match &msg.content {
            Content::Text(text) => text.clone(),
            _ => String::new(),
        };

        let role = match msg.role {
            MessageRole::user => "user",
            MessageRole::assistant => "assistant",
            MessageRole::tool => "tool",
            MessageRole::system => "system",
            MessageRole::function => "tool",
        }
        .to_string();

        let tool_calls = msg.tool_calls.as_ref().map(|calls| {
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
            id,
            role,
            content,
            timestamp,
            tool_call_id: msg.tool_call_id.clone(),
            tool_calls,
            metadata: None,
        }
    }

    /// 从应用层 `ChatMessage` 转换为 `ChatCompletionMessage`，用于调 LLM。
    pub fn to_chat_completion_message(&self) -> ChatCompletionMessage {
        let role = match self.role.as_str() {
            "user" => MessageRole::user,
            "assistant" => MessageRole::assistant,
            "tool" => MessageRole::tool,
            "system" => MessageRole::system,
            _ => MessageRole::user,
        };

        let tool_calls = self.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc.id.clone(),
                    r#type: "function".to_string(),
                    function: ToolCallFunction {
                        name: Some(tc.name.clone()),
                        arguments: Some(tc.arguments.clone()),
                    },
                })
                .collect()
        });

        ChatCompletionMessage {
            role,
            content: Content::Text(self.content.clone()),
            name: None,
            tool_calls,
            tool_call_id: self.tool_call_id.clone(),
        }
    }

    /// 便捷方法：从 `ChatCompletionMessage` 转换，并使用当前时间戳。
    pub fn from_chat_completion_message_now(
        msg: &ChatCompletionMessage,
        id: String,
    ) -> Self {
        Self::from_chat_completion_message(msg, id, Local::now().timestamp())
    }

    /// 便捷方法：生成一条新的 user `ChatMessage`。
    pub fn new_user(id: String, content: String) -> Self {
        Self {
            id,
            role: "user".to_string(),
            content,
            timestamp: Local::now().timestamp(),
            tool_call_id: None,
            tool_calls: None,
            metadata: None,
        }
    }
}
