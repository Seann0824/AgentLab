use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content, MessageRole, ToolCall};

#[derive(Clone)]
pub struct Message {
    pub id: String,
    pub naive_message: ChatCompletionMessage,
    pub timestamp: DateTime<Local>,
    pub metadata: Option<Value>,
}

impl Message {
    // 现在接受 Option<&Value>，如果为 None 则返回 None
    fn get_metadata(kwargs: Option<&Value>) -> Option<Value> {
        kwargs.and_then(|v| v.get("metadata").cloned())
    }

    // 接受 Option<&Value>，如果 timestamp 字段无效或不存在则返回当前时间
    fn get_timestamp(kwargs: Option<&Value>) -> DateTime<Local> {
        kwargs
            .and_then(|v| v.get("timestamp").and_then(|t| t.as_i64()))
            .and_then(|ts| Local.timestamp_opt(ts, 0).single())
            .unwrap_or_else(Local::now)
    }

    fn new_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    // 各个构造器：参数改为 impl Into<Option<Value>>
    pub fn user(content: impl Into<String>, kwargs: impl Into<Option<Value>>) -> Self {
        let kwargs = kwargs.into();
        Self {
            id: Self::new_id(),
            naive_message: ChatCompletionMessage {
                role: MessageRole::user,
                content: Content::Text(content.into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            timestamp: Self::get_timestamp(kwargs.as_ref()),
            metadata: Self::get_metadata(kwargs.as_ref()),
        }
    }

    pub fn assistant(content: impl Into<String>, kwargs: impl Into<Option<Value>>) -> Self {
        let kwargs = kwargs.into();
        Self {
            id: Self::new_id(),
            naive_message: ChatCompletionMessage {
                role: MessageRole::assistant,
                content: Content::Text(content.into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            timestamp: Self::get_timestamp(kwargs.as_ref()),
            metadata: Self::get_metadata(kwargs.as_ref()),
        }
    }

    pub fn assistant_with_tools(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        kwargs: impl Into<Option<Value>>,
    ) -> Self {
        let kwargs = kwargs.into();
        Self {
            id: Self::new_id(),
            naive_message: ChatCompletionMessage {
                role: MessageRole::assistant,
                content: Content::Text(content.into()),
                tool_calls: Some(tool_calls),
                name: None,
                tool_call_id: None,
            },
            timestamp: Self::get_timestamp(kwargs.as_ref()),
            metadata: Self::get_metadata(kwargs.as_ref()),
        }
    }

    pub fn tool_response(
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
        kwargs: impl Into<Option<Value>>,
    ) -> Self {
        let kwargs = kwargs.into();
        Self {
            id: Self::new_id(),
            naive_message: ChatCompletionMessage {
                role: MessageRole::tool,
                content: Content::Text(content.into()),
                tool_call_id: Some(tool_call_id.into()),
                tool_calls: None,
                name: None,
            },
            timestamp: Self::get_timestamp(kwargs.as_ref()),
            metadata: Self::get_metadata(kwargs.as_ref()),
        }
    }

    pub fn system(content: impl Into<String>, kwargs: impl Into<Option<Value>>) -> Self {
        let kwargs = kwargs.into();
        Self {
            id: Self::new_id(),
            naive_message: ChatCompletionMessage {
                role: MessageRole::system,
                content: Content::Text(content.into()),
                tool_call_id: None,
                tool_calls: None,
                name: None,
            },
            timestamp: Self::get_timestamp(kwargs.as_ref()),
            metadata: Self::get_metadata(kwargs.as_ref()),
        }
    }
}