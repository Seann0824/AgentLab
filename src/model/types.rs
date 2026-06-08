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
    System {
        content: String,
    },
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
    pub fn system(content: impl Into<String>) -> Self {
        ChatMessage::System {
            content: content.into(),
        }
    }

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

/// 添加 Send + Sync 约束以支持跨线程使用
pub trait ModelAdapter: Send + Sync {
    fn stream_chat(&self, messages: &[ChatMessage], tools: serde_json::Value) -> ModelStream;
    /// 克隆自身为 Box<dyn ModelAdapter>（用于跨线程共享，如异步摘要器）
    fn clone_box(&self) -> Box<dyn ModelAdapter>;
}

/// 让 Box<dyn ModelAdapter> 支持 clone（委托给 clone_box）
impl Clone for Box<dyn ModelAdapter> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub type ModelStream = Pin<Box<dyn Stream<Item = ModelEvent> + Send + 'static>>;
