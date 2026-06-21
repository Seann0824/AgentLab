use std::pin::Pin;
use std::io::Read;
pub mod simple_agent;

use futures_util::{Stream, StreamExt};
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content, MessageRole::{self, user}, chat_completion_stream::ChatCompletionStreamResponse};

use crate::{model::openai, tools::ToolManager};
pub struct AssistantAgent {
    name: String,
    model_client: openai::OpenaiChatCompletionClient,
    system_message: String,
    tool_manager: ToolManager,
}

impl AssistantAgent {
    pub fn new(name: String, model_client: openai::OpenaiChatCompletionClient, system_message: String, tool_manager: ToolManager) -> Self {
        Self {
            name,
            model_client,
            system_message,
            tool_manager,
        }
    }

    pub async fn run(&self, history_message: &Vec<ChatCompletionMessage>) -> Vec<ChatCompletionMessage> {
        println!("AssistantAgent {}", self.name);
        let mut copy_history_message = history_message.clone();
        copy_history_message.insert(
            0,
            ChatCompletionMessage { 
                role: MessageRole::system, 
                content: Content::Text(self.system_message.clone()), 
                name: None, tool_calls: None, tool_call_id: None 
            }
        );

        let tools_schema = self.tool_manager.get_tools_scehma();
        let mut generated_messages = Vec::new();
        for _ in 0..8 {
            let think_stream = self.model_client.think(copy_history_message.clone(), Some(tools_schema.clone()), None).await;
            let (mut current_messages, should_continue) = process_think_stream(Box::pin(think_stream), &self.tool_manager).await;
            copy_history_message.extend(current_messages.clone());
            generated_messages.append(&mut current_messages);
            if !should_continue {
                break;
            }
        }

        generated_messages
    }
}


async fn process_think_stream(mut think_stream: Pin<Box<impl Stream<Item = ChatCompletionStreamResponse>>>, tool_manager: &ToolManager) -> (Vec<ChatCompletionMessage>, bool) {
    let mut messages = vec![];
    let (mut is_first_print_content, mut is_first_print_reason) = (true, true);

    let (mut reason_delta, mut content_delta) = (vec![], vec![]);
    let mut tools_call = None; 

    while let Some(chunck) = think_stream.next().await {
        match chunck {
            ChatCompletionStreamResponse::Content(delta) => {
                if is_first_print_content {
                    println!("\n\n");
                    is_first_print_content = false;
                }
                print!("{}", delta);
                content_delta.push(delta);

            },
            ChatCompletionStreamResponse::Reasoning(delta) => {
                if is_first_print_reason {
                    println!("\n\n");
                    is_first_print_reason = false;
                }
                // print!("{}", delta);
                reason_delta.push(delta);
            },
            ChatCompletionStreamResponse::ToolCall(tc) => {
                tools_call = Some(tc);
            },
            ChatCompletionStreamResponse::Done(_finish_reason)=> {
                let should_continue = tools_call.is_some();
                // 区分调用工具和没有调用工具的信息
                // tool call
                if let Some(tools_call) = &tools_call {
                    
                    let tasks = tools_call
                        .iter()
                        .map(|tool_call| tool_manager.run(tool_call.clone()))
                        .collect::<Vec<_>>();
                    
                    let tools_call_result = futures_util::future::join_all(tasks).await;
                    // 工具调用
                    messages.push(
                        ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(content_delta.join("")), tool_calls: Some(tools_call.clone()), name: None, tool_call_id: None }
                    );
                    // 工具调用结果
                    tools_call_result
                        .into_iter()
                        .for_each(|(tool_call_id, tool_call_result)| {
                            let tool_call_result = match tool_call_result {
                                Ok(content) => content,
                                Err(error_msg) => error_msg,
                            };
                            println!("tool_call_result: {}", tool_call_result);
                            messages.push(
                                ChatCompletionMessage { role: MessageRole::tool, content: Content::Text(tool_call_result), tool_call_id: Some(tool_call_id), name: None, tool_calls: None }
                            )
                        });     
                } else {
                    messages.push(
                        ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(content_delta.join("")), name: None, tool_calls: None, tool_call_id: None }
                    );
                }

                return (messages, should_continue);
            },
        }
        std::io::Write::flush(&mut std::io::stdout());
    }

    (messages, false)
}


pub struct UserProxyAgent {
    name: String,
    description: String,
}

impl UserProxyAgent {
    pub fn new(name: String, description: String) -> Self {
        Self {
            name,
            description
        }
    }

    pub async fn run(&self) -> Vec<ChatCompletionMessage> {
        let mut messages: Vec<ChatCompletionMessage> = vec![];
        let mut user_input = String::new();
        let _ = std::io::stdin().read_line(&mut user_input);

        messages.push(
            ChatCompletionMessage { role: MessageRole::user, content: Content::Text(user_input), name: None, tool_calls: None, tool_call_id: None }
        );

        messages
    }
}

pub enum Agent {
    AssistantAgent(AssistantAgent),
    UserProxyAgent(UserProxyAgent)
}
