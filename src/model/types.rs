use std::pin::Pin;

use futures_util::Stream;
use serde::Serialize;

use crate::tools::types::Tool;


#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}


#[derive(Debug, Clone)]
enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug)]
pub enum ModelEvent {
    Text(String),
    Thinking(String),
    // ToolCallDelta {
    //     id: String,
    //     name: Option<String>,
    //     arguments_delta: String,
    // },
    Done,
    Error(String),
}

pub trait ModelAdapter {
    fn stream_chat(&self, messages: Vec<ChatMessage>, tools: Option<Vec<Box<dyn Tool>>>) -> ModelStream;
}

pub type ModelStream = Pin<Box<dyn Stream<Item = ModelEvent> + Send + 'static>>;