use std::env;
use dotenvy;
use openai_api_rs::v1::chat_completion::chat_completion_stream::ChatCompletionStreamResponse;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content};

use openai_api_rs::v1::chat_completion::MessageRole;

use crate::tools::web_search::WebSearch;
use crate::tools::ToolManager;
mod model;
mod tools;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> () {
    dotenvy::dotenv().ok();

    let api_key = env::var("DEEPSEEK_API_KEY").unwrap();
    let base_url = env::var("DEEPSEEK_BASE_URL").unwrap();
    let tool_manager = ToolManager::new()
        .register_tool(Box::new(WebSearch::new()));

    let llm_client = model::AgentLLM::new("deepseek-v4-pro", &api_key, &base_url, None);
    
    let mut messages: Vec<ChatCompletionMessage> = vec![
        ChatCompletionMessage {
            role: MessageRole::system,
            content: Content::Text("You are a helpful assistant that writes Python code.".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatCompletionMessage {
            role: MessageRole::user,
            content: Content::Text("帮我查查伦敦天气, 记得传递 query".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    ];

    let mut count = 2;
    loop {
        if count <= 0 {
            break;
        }
        count -= 1;
        let mut think_stream = llm_client.think(messages.clone(), Some(tool_manager.get_tools_scehma()), None).await;
        
        let (mut is_first_print_content, mut is_first_print_reason) = (true, true);


        let (mut reason_delta, mut content_delta) = (vec![], vec![]);
        let mut tools_call = None; 

        while let Some(chunck) = think_stream.next().await {
            match chunck {
                ChatCompletionStreamResponse::Content(delta) => {
                    if is_first_print_content {
                        println!("\n\nAI: ");
                        is_first_print_content = false;
                    }
                    print!("{}", delta);
                    content_delta.push(delta);

                },
                ChatCompletionStreamResponse::Reasoning(delta) => {
                    if is_first_print_reason {
                        println!("\n\nTHINK: ");
                        is_first_print_reason = false;
                    }
                    print!("{}", delta);
                    reason_delta.push(delta);
                },
                ChatCompletionStreamResponse::ToolCall(tc) => {
                    tools_call = Some(tc);
                },
                ChatCompletionStreamResponse::Done=> {
                    // 区分调用工具和没有调用工具的信息

                    // message 处理，工具调用处理（工具本身调用也可以作为一个流，但是本次就先做简单版本）
                    messages.push(
                        ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(reason_delta.join("")), name: None, tool_calls: None, tool_call_id: None },
                    );
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
                                messages.push(
                                    ChatCompletionMessage { role: MessageRole::tool, content: Content::Text(tool_call_result), tool_call_id: Some(tool_call_id), name: None, tool_calls: None }
                                )
                            });     
                    } else {
                        messages.push(
                            ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(content_delta.join("")), name: None, tool_calls: None, tool_call_id: None }
                        );
                    }
                },
            }

            std::io::Write::flush(&mut std::io::stdout());
        }
    }


}
