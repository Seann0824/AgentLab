use std::env;

use crate::base::{agent::{Agent, AgentBase}, config::Config, llm::AgentsLLM, message::Message};

struct SimpleAgent {
    base: AgentBase,
}

impl SimpleAgent {
    pub fn new() -> Self {
        let config = Config::from_env();
        let llm = AgentsLLM::get_agents_llm_instance();
  
        let agent_base = AgentBase::new(
            "SimpleAgent", 
            llm, 
            Some(Self::get_system_prompt().into()), 
            Some(config),
        );
        Self {
            base: agent_base,
        }
    }

    fn get_system_prompt() -> &'static str {
        r#""#
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

    }
}
