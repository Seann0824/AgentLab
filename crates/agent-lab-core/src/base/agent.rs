use tokio::sync::mpsc;

use openai_api_rs::v1::chat_completion::{
    ChatCompletionMessage, Content, MessageRole, ToolCall,
};

use crate::base::config::Config;
use crate::base::llm::AgentsLLM;
use crate::services::chat_dto::ChatMessage;

pub struct AgentBase {
    pub name: String,
    pub llm: AgentsLLM,
    pub system_prompt: Option<String>,
    pub config: Config,
    history: Vec<ChatCompletionMessage>,
    event_sender: Option<mpsc::Sender<AgentStreamEvent>>,
}

impl AgentBase {
    pub fn new(
        name: impl Into<String>,
        llm: AgentsLLM,
        system_prompt: Option<String>,
        config: Option<Config>,
    ) -> Self {
        let history = if system_prompt.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
            vec![system_message(system_prompt.clone().unwrap())]
        } else {
            vec![]
        };
        Self {
            name: name.into(),
            llm,
            system_prompt,
            config: config.unwrap_or_default(),
            history,
            event_sender: None,
        }
    }

    pub fn add_message(&mut self, message: ChatCompletionMessage) {
        self.history.push(message);
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn get_history(&self) -> Vec<ChatCompletionMessage> {
        self.history.clone()
    }

    pub fn set_history(&mut self, history: Vec<ChatCompletionMessage>) {
        self.history = history;
    }

    pub fn set_event_sender(&mut self, tx: Option<mpsc::Sender<AgentStreamEvent>>) {
        self.event_sender = tx;
    }

    /// 确保历史记录以 system prompt 开头。
    /// 如果传入的历史中已包含 system 消息，则不再重复添加。
    pub fn ensure_system_prompt(&mut self) {
        if let Some(sp) = &self.system_prompt {
            if !sp.is_empty() {
                let has_system = self
                    .history
                    .first()
                    .map(|m| m.role == MessageRole::system)
                    .unwrap_or(false);
                if !has_system {
                    self.history.insert(0, system_message(sp.clone()));
                }
            }
        }
    }

    pub async fn emit(&self, event: AgentStreamEvent) {
        if let Some(tx) = &self.event_sender {
            let _ = tx.send(event).await;
        }
    }
}

/// 构造 user 角色的 `ChatCompletionMessage`。
pub fn user_message(content: impl Into<String>) -> ChatCompletionMessage {
    ChatCompletionMessage {
        role: MessageRole::user,
        content: Content::Text(content.into()),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

/// 构造 assistant 角色的 `ChatCompletionMessage`。
pub fn assistant_message(content: impl Into<String>) -> ChatCompletionMessage {
    ChatCompletionMessage {
        role: MessageRole::assistant,
        content: Content::Text(content.into()),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

/// 构造带 tool_calls 的 assistant `ChatCompletionMessage`。
pub fn assistant_message_with_tools(
    content: impl Into<String>,
    tool_calls: Vec<ToolCall>,
) -> ChatCompletionMessage {
    ChatCompletionMessage {
        role: MessageRole::assistant,
        content: Content::Text(content.into()),
        name: None,
        tool_calls: Some(tool_calls),
        tool_call_id: None,
    }
}

/// 构造 tool 角色的 `ChatCompletionMessage`。
pub fn tool_message(
    tool_call_id: impl Into<String>,
    content: impl Into<String>,
) -> ChatCompletionMessage {
    ChatCompletionMessage {
        role: MessageRole::tool,
        content: Content::Text(content.into()),
        name: None,
        tool_calls: None,
        tool_call_id: Some(tool_call_id.into()),
    }
}

/// 构造 system 角色的 `ChatCompletionMessage`。
pub fn system_message(content: impl Into<String>) -> ChatCompletionMessage {
    ChatCompletionMessage {
        role: MessageRole::system,
        content: Content::Text(content.into()),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentStreamEvent {
    /// 用户消息已加入历史。
    UserMessage { message: ChatMessage },

    /// assistant 流式内容增量。
    AssistantDelta { message_id: String, delta: String },

    /// assistant 消息生成完毕（可能携带 tool_calls）。
    AssistantDone { message: ChatMessage },

    /// 某个工具调用开始执行。
    ToolCallStart {
        tool_call_id: String,
        tool_name: String,
        arguments: String,
    },

    /// 某个工具调用结束并返回结果。
    ToolCallEnd {
        tool_call_id: String,
        tool_name: String,
        result: String,
        is_error: bool,
    },

    /// 工具结果流式增量（预留）。
    ToolCallDelta {
        tool_call_id: String,
        delta: String,
    },

    /// reasoning 增量。
    ReasonDelta { message_id: String, delta: String },

    /// reasoning 完成。
    ReasonDone { reason: String },
}

#[async_trait::async_trait]
pub trait Agent: Send + Sync {
    fn base(&self) -> &AgentBase;
    fn base_mut(&mut self) -> &mut AgentBase;

    async fn run(&mut self, input_text: &str) -> String;

    fn add_message(&mut self, message: ChatCompletionMessage) {
        self.base_mut().add_message(message);
    }

    fn clear_history(&mut self) {
        self.base_mut().clear_history();
    }

    fn get_history(&self) -> Vec<ChatCompletionMessage> {
        self.base().get_history()
    }

    fn description(&self) -> String {
        format!(
            "Agent(name={}, provider={})",
            self.base().name,
            self.base().llm.provider
        )
    }
}
