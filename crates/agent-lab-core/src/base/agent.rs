use crate::base::config::Config;
use crate::base::message::Message;
use crate::base::llm::AgentsLLM;

pub struct AgentBase {
    pub name: String,
    pub llm: AgentsLLM,
    pub system_prompt: Option<String>,
    pub config: Config,
    history: Vec<Message>,
}

impl AgentBase {
    pub fn new(
        name: impl Into<String>,
        llm: AgentsLLM,
        system_prompt: Option<String>,
        config: Option<Config>,
    ) -> Self {
        let history = vec![Message::system(system_prompt.clone().unwrap_or("".into()), None)];
        Self {
            name: name.into(),
            llm,
            system_prompt,
            config: config.unwrap_or_default(),
            history,
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