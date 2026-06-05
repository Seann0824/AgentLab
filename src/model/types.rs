use serde::Serialize;


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

#[async_trait::async_trait]
pub trait ModelAdapter {
    async fn stream_chat(&self, messages: Vec<ChatMessage>) -> anyhow::Result<()>;
}