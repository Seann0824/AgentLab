use std::env;
use chrono::Weekday::Tue;
use futures_util::stream::StreamExt;
use openai_api_rs::v1::chat_completion::{FinishReason, MessageRole::user, chat_completion_stream::ChatCompletionStreamResponse};

use crate::{base::{agent::{Agent, AgentBase}, config::Config, llm::AgentsLLM, message::Message}, tools::{ToolManager, web_search::WebSearch}};

struct SimpleAgent {
    tool_manager: ToolManager,
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

        let tool_manager = Self::get_tool_manager();
        
        Self {
            tool_manager,
            base: agent_base,
        }
    }

    fn get_system_prompt() -> &'static str {
        r#""#
    }

    fn get_tool_manager() -> ToolManager {
        // 这里注册当前Agent有的工具
        ToolManager::new()
            .register_tool(Box::new(WebSearch::new()))
    }
}

impl Agent for SimpleAgent {
    fn base(&self) -> &AgentBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AgentBase {
        &mut self.base
    }

    async fn run(&mut self, input_text: &str) -> String {
        print!("🤖 {} 正在处理: {input_text}", self.base.name);
        let user_message = Message::user(input_text, None);
        self.add_message(user_message);
        let mut is_continue = true;

        loop {
            if !is_continue {
                break;
            }
            let history_message = self.base
                .get_history()
                .into_iter()
                .map(|message| message.naive_message)
                .collect();
            
            let mut agent_stream = self.base.llm.invoke(history_message, Some(self.tool_manager.get_tools_scehma()), None).await;
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
                    },
                }
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            println!();
        }

        "".into()
    }
}
