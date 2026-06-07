use std::pin::Pin;

use futures_util::Stream;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User {
        content: String,
    },
    Assistant {
        content: String,
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        ChatMessage::User {
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        ChatMessage::Assistant {
            content: content.into(),
            tool_calls: Vec::new(),
        }
    }

    pub fn assistant_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        ChatMessage::Assistant {
            content: content.into(),
            tool_calls,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        ChatMessage::Tool {
            tool_call_id: tool_call_id.into(),
            content: content.into(),
        }
    }

    pub fn tool_call_id(&self) -> Option<&str> {
        match self {
            ChatMessage::Tool { tool_call_id, .. } => Some(tool_call_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ModelEvent {
    Text(String),
    Thinking(String),
    ToolCallBlock {
        id: String, // model 分配，需要回复的时候返回
        name: String,
        arguments: String,
    },
    Done(String),
    Error(String),
}

pub trait ModelAdapter {
    fn stream_chat(&self, messages: &Vec<ChatMessage>, tools: serde_json::Value) -> ModelStream;
}

pub type ModelStream = Pin<Box<dyn Stream<Item = ModelEvent> + Send + 'static>>;
