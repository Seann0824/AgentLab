use crate::base::{agent::{Agent, AgentBase}, config::Config, llm::OpenaiChatCompletionClient};

struct SimpleAgent {
    base: AgentBase,
}

impl SimpleAgent {
    pub fn new(
        name: impl Into<String>,
        llm: OpenaiChatCompletionClient,
        system_prompt: Option<String>,
        config: Option<Config>
    ) -> Self {
        let agent_base = AgentBase::new(name, llm, system_prompt, config);
        Self {
            base: agent_base,
        }
    }
}

impl Agent for SimpleAgent {
    fn base(&self) -> &AgentBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AgentBase {
        &mut self.base
    }

    fn run(&mut self, input_text: &str) -> String {
        todo!()
    }
}
