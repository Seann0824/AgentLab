use openai_api_rs::v1::chat_completion::{FinishReason, chat_completion_stream::ChatCompletionStreamResponse};

use crate::{base::{agent::{Agent, AgentBase, }, config::Config, llm::AgentsLLM, message::Message }, tools::ToolManager};
use std::io::{self, Write};
use futures_util::stream::StreamExt;

pub struct ReflectionAgent {
    base: AgentBase,
    tool_manager: ToolManager,
    prompt_templates: Option<ReflectionPromptTemplates>,
    max_steps: u64,
}

#[derive(Clone)]
pub struct ReflectionPromptTemplates {
    pub initial: String,
    pub reflect: String,
    pub refine: String,
}

impl ReflectionPromptTemplates {
    fn iter(&self) -> [(&str, &str); 3] {
        [
            ("initial", &self.initial),
            ("reflect", &self.reflect),
            ("refine", &self.refine),
        ]
    }
}

impl Default for ReflectionPromptTemplates {
    fn default() -> Self {
        Self { initial: Default::default(), reflect: Default::default(), refine: Default::default() }
    }
}

impl ReflectionAgent {
    pub fn new(
        name: impl Into<String>, 
        llm: AgentsLLM,
        prompt_templates: impl Into<Option<ReflectionPromptTemplates>>,
        config: impl Into<Option<Config>>,
        tool_manager: impl Into<Option<ToolManager>>,
        max_steps: impl Into<Option<u64>>,
    ) -> Self {
        let config = config.into().unwrap_or_default();
        let tool_manager = tool_manager.into().unwrap_or(ToolManager::new());
        let max_steps = max_steps.into().unwrap_or(5);
        let prompt_templates = prompt_templates.into();
        let agent_base = AgentBase::new(
            name.into(),
            llm, 
            Some("".to_string()), 
            Some(config),
        );

        Self {
            tool_manager,
            base: agent_base,
            max_steps,
            prompt_templates,
        }
    }

    pub async fn run_reflection(&mut self, task: &str) -> String {
        let Some(prompt_templates) = self.prompt_templates.clone() else {
            return self.run(task).await;
        };

        let mut last_result: Option<String> = None;
        let mut final_result = String::new();
        self.base.clear_history();

        for (_key, template) in prompt_templates.iter() {
            let mut prompt = template.replace("{task}", task);

            if let Some(content) = &last_result {
                prompt = prompt.replace("{content}", content);
            }

            final_result = self.run(&prompt).await;
            last_result = Some(final_result.clone());
        }

        final_result
    }
}

#[async_trait::async_trait]
impl Agent for ReflectionAgent {
    fn base(&self) -> &AgentBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AgentBase {
        &mut self.base
    }

    async fn run(&mut self, input_text: &str) -> String {
        println!("🤖 {} 正在处理: {input_text}", self.base.name);
        io::stdout().flush().ok();
        let user_message = Message::user(input_text, None);
        self.add_message(user_message);
        let mut is_continue = true;
        let mut current_step = 0u64;
        let mut final_resut = String::new();
        loop {
            if !is_continue {
                break;
            }
            let history_message = self.base
                .get_history()
                .into_iter()
                .map(|message| message.naive_message)
                .collect();
            let mut agent_stream = self.base.llm.invoke(
                history_message, 
                Some(self.tool_manager.get_tools_scehma()),
                None
            ).await;
            let (mut reason_delta, mut content_delta) = (vec![], vec![]);
            let mut tools_call = None;

            while let Some(chunck) = agent_stream.next().await {
                match chunck {
                    ChatCompletionStreamResponse::Content(delta) => {
                        print!("{}", delta);
                        content_delta.push(delta);

                    },
                    ChatCompletionStreamResponse::Reasoning(delta) => {
                        reason_delta.push(delta);
                    },
                    ChatCompletionStreamResponse::ToolCall(tc) => {
                        tools_call = Some(tc);
                    },
                    ChatCompletionStreamResponse::Done(finish_reason)=> {
                        // 区分调用工具和没有调用工具的信息
                        // 1. 判断当时是否是工具调用
                        let Some(finish_reason) = finish_reason else {
                            panic!("end failed");
                        };

                        match finish_reason {
                            FinishReason::tool_calls | FinishReason::function_call => {
                                let tools_call = tools_call.clone().unwrap_or_default().clone();
                                self.add_message(Message::assistant(reason_delta.join(""), None));
                                self.add_message(Message::assistant_with_tools(content_delta.join(""), tools_call.clone(),None));
                                // 工具调用开始
                                let tasks = tools_call
                                    .iter()
                                    .map(|tool_call| self.tool_manager.run(tool_call.clone()))
                                    .collect::<Vec<_>>();
                                
                                let tools_call_result = futures_util::future::join_all(tasks).await;
                                
                                tools_call_result
                                    .into_iter()
                                    .for_each(|(tool_call_id, tool_call_result)| {
                                        let tool_call_result = match tool_call_result {
                                            Ok(content) => content,
                                            Err(error_msg) => error_msg,
                                        };
                                        println!("tool_call_result: {}", tool_call_result);
                                        self.add_message(Message::tool_response(tool_call_id, tool_call_result, None));
                                    });
                            },
                            _ => {
                                self.add_message(Message::assistant(reason_delta.join(""), None));
                                self.add_message(Message::assistant(content_delta.join(""), None));
                                final_resut = content_delta.join("");
                                is_continue = false;
                                break;
                            },
                        }

                        current_step += 1;
                        if current_step == self.max_steps {
                            is_continue = false;
                            println!("抱歉，我无法在限定步数内完成这个任务。");
                            let _ = std::io::Write::flush(&mut std::io::stdout());
                        }
                    },
                }
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            println!();
        }

        final_resut
    }

    fn add_message(&mut self, message: crate::base::message::Message) {
        self.base_mut().add_message(message);
    }
    
    fn clear_history(&mut self) {
        self.base_mut().clear_history();
    }
    
    fn get_history(&self) -> Vec<crate::base::message::Message> {
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
