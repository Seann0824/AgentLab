use tokio::sync::mpsc;

use crate::base::config::Config;
use crate::base::llm::AgentsLLM;
use crate::base::message::Message;

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
        let history = vec![Message::system(
            system_prompt.clone().unwrap_or("".into()),
            None,
        )];
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
pub enum AgentStreamEvent {
    Content {
        delta: String,
    },
    Reason {
        delta: String,
    },
    ContentDone {
        content: String,
    },
    ReasonDone {
        reason: String,
    },
    ToolCall {
        tool_name: String,
        tool_call_id: String,
    },
    ToolCallResult {
        is_error: bool,
        tool_name: String,
        tool_call_id: String,
        tool_call_result: String,
    },
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
