use tokio::sync::mpsc;

use crate::base::config::Config;
use crate::base::llm::AgentsLLM;
use crate::base::message::Message;
use crate::services::chat_dto::ChatMessage;

pub struct AgentBase {
    pub name: String,
    pub llm: AgentsLLM,
    pub system_prompt: Option<String>,
    pub config: Config,
    history: Vec<Message>,
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
            vec![Message::system(system_prompt.clone().unwrap(), None)]
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

    pub fn add_message(&mut self, message: Message) {
        self.history.push(message);
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn get_history(&self) -> Vec<Message> {
        self.history.clone()
    }

    pub fn set_event_sender(&mut self, tx: Option<mpsc::Sender<AgentStreamEvent>>) {
        self.event_sender = tx;
    }

    pub async fn emit(&self, event: AgentStreamEvent) {
        if let Some(tx) = &self.event_sender {
            let _ = tx.send(event).await;
        }
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

    /// 某个工具调用执行结束并返回结果。
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
    ReasonDelta { delta: String },

    /// reasoning 完成。
    ReasonDone { reason: String },
}

#[async_trait::async_trait]
pub trait Agent: Send + Sync {
    fn base(&self) -> &AgentBase;
    fn base_mut(&mut self) -> &mut AgentBase;

    async fn run(&mut self, input_text: &str) -> String;

    fn add_message(&mut self, message: Message) {
        self.base_mut().add_message(message);
    }

    fn clear_history(&mut self) {
        self.base_mut().clear_history();
    }

    fn get_history(&self) -> Vec<Message> {
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
