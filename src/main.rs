use std::env;
use std::mem::take;
use dotenvy;
use openai_api_rs::v1::chat_completion::chat_completion_stream::ChatCompletionStreamResponse;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content};

use openai_api_rs::v1::chat_completion::MessageRole;

use crate::tools::web_search::WebSearch;
use crate::tools::{ToolManager, web_search};
mod model;
mod tools;
mod agent;
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
            content: Content::Text("100字表白".into()),
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
        let tools_call = None; 

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
                    // message 处理，工具调用处理（工具本身调用也可以作为一个流，但是本次就先做简单版本）
                    messages.push(
                        ChatCompletionMessage { role: MessageRole::assistant, content: reason_delta, name: None, tool_calls: None, tool_call_id: None },
                    );
                    messages.push(
                        ChatCompletionMessage { role: MessageRole::assistant, content: content_delta, name: None, tool_calls: None, tool_call_id: None }
                    );

                    // 工具调用

                    // tool call
                    if let Some(tools_call) = tools_call {
                        let tasks = tools_call
                            .iter()
                            .map(|tool_call| tool_manager.run(tool_call.clone()))
                            .collect::<Vec<_>>();
                        
                        let tools_call_result = futures_util::future::join_all(tasks).await;
                        // make tool_call message
                        
                    }
                },
            }

            std::io::Write::flush(&mut std::io::stdout());
        }
    }


}
