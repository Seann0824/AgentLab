use std::io::{self, Write};
use futures_util::stream::StreamExt;
use openai_api_rs::v1::chat_completion::{FinishReason, chat_completion_stream::ChatCompletionStreamResponse};
use crate::{base::{agent::{Agent, AgentBase}, config::Config, llm::AgentsLLM, message::Message}, tools::{ToolManager, types::Tool, web_search::WebSearch}};

pub struct SimpleAgent {
    enable_tool_calling: bool,
    tool_manager: ToolManager,
    base: AgentBase,
}

impl SimpleAgent {
    pub fn new(
        name: impl Into<String>, 
        llm: AgentsLLM,
        system_prompt: impl Into<Option<String>>,
        config: impl Into<Option<Config>>,
        tool_manager: impl Into<Option<ToolManager>>,
        enable_tool_calling: bool
    ) -> Self {
        let config = config.into().unwrap_or(Config::from_env());
        let system_prompt = system_prompt.into().unwrap_or("".into());
        let tool_manager = tool_manager.into().unwrap_or(ToolManager::new());
        let agent_base = AgentBase::new(
            name.into(),
            llm, 
            Some(system_prompt.clone()), 
            Some(config),
        );

        Self {
            enable_tool_calling,
            tool_manager,
            base: agent_base,
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

impl Agent for SimpleAgent {
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
        let mut final_response = String::new();

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
                self.enable_tool_calling.then(|| self.tool_manager.get_tools_scehma()),
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
                                // 如果模型声明要调用工具但没有给出具体 tool_call，降级为普通内容回复。
                                if tools_call.is_empty() {
                                    let content = content_delta.join("");
                                    if !content.is_empty() {
                                        self.add_message(Message::assistant(content.clone(), None));
                                        final_response = content;
                                    }
                                    is_continue = false;
                                    break;
                                }
                                // 部分模型/提供商不接受 content 和 tool_calls 同时为空的 assistant 消息。
                                // 如果 reasoning 内容为空，则不加入历史，避免后续请求被 API 拒绝。
                                let reasoning = reason_delta.join("");
                                if !reasoning.is_empty() {
                                    self.add_message(Message::assistant(reasoning, None));
                                }
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
                                let content = content_delta.join("");
                                let reasoning = reason_delta.join("");
                                if !reasoning.is_empty() {
                                    self.add_message(Message::assistant(reasoning, None));
                                }
                                self.add_message(Message::assistant(content.clone(), None));
                                final_response = content;
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

        final_response
    }
}
