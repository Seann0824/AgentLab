use std::env;
use anyhow;
use dotenvy;
use futures_util::{StreamExt, stream::FuturesUnordered};

use crate::{model::{ChatMessage, ModelEvent, ToolCall}, tools::{ToolManager, read_file::ReadFile}};

mod model;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let query_client = initial_model()?;
    let tool_manager = initial_tool_manager();
    let mut messages: Vec<ChatMessage> = vec![];
    let mut is_auto = false;
   
    loop {
        if !is_auto {
            let mut user_input = String::new();
            print!(">");
            std::io::Write::flush(&mut std::io::stdout())?;
            if std::io::stdin().read_line(&mut user_input).is_err() {
                continue;
            }
            if user_input.trim().is_empty() {
                continue;
            }
            messages.push(ChatMessage::user(user_input));
        }

        let mut stream_chat = query_client.stream_chat(&messages, tool_manager.get_tools_scehma());
        let mut tool_tasks = FuturesUnordered::new();
        let mut tool_calls = Vec::new();
        let mut assistant_content = String::new();
        let mut final_assistant_message = String::new();
        while let Some(model_event)  = stream_chat.next().await {
            match model_event {
                ModelEvent::Text(content) => {
                    assistant_content.push_str(&content);
                    print!("{}", content);
                },
                ModelEvent::Thinking(content) => {
                    print!("\x1b[90m{}\x1b[0m", content);
                },
                ModelEvent::ToolCallBlock { id, name, arguments } => {
                    let tool_call = ToolCall { id, name, arguments };
                    tool_calls.push(tool_call.clone());
                    tool_tasks.push(tool_manager.run(tool_call));
                },
                ModelEvent::Done(assistant_message) => {
                    // 还要把Done的内容返回一下。。。              
                    // 先把 assistant 信息返回 保存
                    final_assistant_message = assistant_message;
                }
                _ => ()
            }
            std::io::Write::flush(&mut std::io::stdout())?;
        }

        let tool_call_ids = tool_calls
            .iter()
            .map(|tool_call| tool_call.id.clone())
            .collect::<Vec<_>>();
        if tool_calls.len() > 0 {
            messages.push(ChatMessage::assistant_tool_calls(assistant_content, tool_calls));
            is_auto = true;
        } else {
            messages.push(ChatMessage::assistant(final_assistant_message));
            is_auto = false;
        }
        let mut tool_results = Vec::new();
        while let Some(tool_result) = tool_tasks.next().await {
            tool_results.push(tool_result);
        }
        for tool_call_id in tool_call_ids {
            if let Some(index) = tool_results
                .iter()
                .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
            {
                let tool_result = tool_results.remove(index);
                if let ChatMessage::Tool { content, .. } = &tool_result {
                    print!("{}", content);
                }
                messages.push(tool_result);
            }
        }
        for tool_result in tool_results {
            if let ChatMessage::Tool { content, .. } = &tool_result {
                print!("{}", content);
            }
            messages.push(tool_result);
        }
    }
}


fn initial_model() -> anyhow::Result<Box<dyn model::ModelAdapter>> {
    // 1. 读取环境变量
    dotenvy::dotenv().ok();

    let api_key = env::var("DEEPSEEK_API_KEY").unwrap();
    let deepseek_base_url = env::var("DEEPSEEK_BASE_URL").unwrap();

    let openai_adapter =
        model::OpenAiCompatibleAdapter::new(
            deepseek_base_url,
            api_key,
            "deepseek-v4-flash".to_string()
        );

    Ok(Box::new(openai_adapter))
}

fn initial_tool_manager() -> ToolManager {
    let mut tool_manager = ToolManager::new();
    tool_manager.register_tool(Box::new(ReadFile));

    tool_manager
}
