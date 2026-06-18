use std::path::Component::CurDir;

use futures_util::Stream;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content, MessageRole};

use crate::agent::{self, Agent};

pub struct TextMentionTermination(pub String);

pub struct RoundRobinGroupChat {
    participants: Vec<agent::Agent>,
    termination_condition: TextMentionTermination,
    max_turns: usize,
    history_messages: Vec<ChatCompletionMessage>,
}

impl RoundRobinGroupChat {
    pub fn new(
        participants: Vec<agent::Agent>,
        termination_condition: TextMentionTermination,
        max_turns: usize
    ) -> Self {
        Self { participants, termination_condition, max_turns, history_messages: vec![] }
    }

    pub async fn run_stream(&mut self, task: &str) {
        self.history_messages.push(
            ChatCompletionMessage { role: MessageRole::user, content: Content::Text(task.into()), name: None, tool_calls: None, tool_call_id: None }
        );
        loop {
            for agent in &self.participants {
                match agent {
                    Agent::AssistantAgent(assistant_agent) => {
                        let mut current_messages = assistant_agent.run(&self.history_messages).await;
                        self.history_messages.append(&mut current_messages);
                    },
                    Agent::UserProxyAgent(user_agent) => {
                        let mut current_messages = user_agent.run().await;
                        if let Some(user_input) = current_messages.get(0) {
                            match user_input {
                                ChatCompletionMessage { content, .. } => {
                                    if let Content::Text(text) = content && text.contains(&self.termination_condition.0) {
                                        return;
                                    }
                                }
                            }
                        }
                        self.history_messages.append(&mut current_messages);
                    }
                }
            }
        }
    }
}

