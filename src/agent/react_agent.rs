use std::io::{self, Write};
use futures_util::stream::StreamExt;
use openai_api_rs::v1::chat_completion::{FinishReason, chat_completion_stream::ChatCompletionStreamResponse};
use crate::{base::{agent::{Agent, AgentBase}, config::Config, llm::AgentsLLM, message::Message}, tools::{ToolManager, types::Tool}};

pub struct ReActAgent {
    base: AgentBase,
    tool_manager: ToolManager,
    max_steps: u64,
}

impl ReActAgent {
    pub fn new(
        name: impl Into<String>, 
        llm: AgentsLLM,
        system_prompt: impl Into<Option<String>>,
        config: impl Into<Option<Config>>,
        tool_manager: impl Into<Option<ToolManager>>,
        max_steps: impl Into<Option<u64>>,
    ) -> Self {
        let config = config.into().unwrap_or(Config::from_env());
        let system_prompt = system_prompt.into().unwrap_or("".into());
        let tool_manager = tool_manager.into().unwrap_or(ToolManager::new());
        let max_steps = max_steps.into().unwrap_or(5);
        let agent_base = AgentBase::new(
            name.into(),
            llm, 
            Some(system_prompt.clone()), 
            Some(config),
        );

        Self {
            tool_manager,
            base: agent_base,
            max_steps,
        }
    }

    // 开放给外部注册工具
    pub fn add_tool(&mut self, tool: Box<dyn Tool + Send + Sync>) {
        self.tool_manager.register_tool(tool);
    }

    pub fn remove_tool(&mut self, tool_name: &String) {
        self.tool_manager.remove_tool(tool_name);
    }

}

#[async_trait::async_trait]
impl Agent for ReActAgent {
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

        "".into()
    }
}
