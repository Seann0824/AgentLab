use std::env;
use dotenvy;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content};

use openai_api_rs::v1::chat_completion::MessageRole;
mod model;
mod tools;

#[tokio::main]
async fn main() -> () {
    dotenvy::dotenv().ok();
    let api_key = env::var("DEEPSEEK_API_KEY").unwrap();
    let base_url = env::var("DEEPSEEK_BASE_URL").unwrap();
    let llm_client = model::AgentLLM::new("deepseek-v4-pro", &api_key, &base_url, None);
    
    let messages: Vec<ChatCompletionMessage> = vec![
        ChatCompletionMessage {
            role: MessageRole::system,
            content: Content::Text("You are a helpful assistant that writes Python code.".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatCompletionMessage {
            role: MessageRole::user,
            content: Content::Text("写一个快速排序算法".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    ]; 

    let response = llm_client.think(messages, None).await;
    if let Ok(text) = response {
        println!("{}", text);
    }
}





// #[tokio::main]
// async fn main() -> anyhow::Result<()> {
//     let query_client = initial_model()?;
//     let tool_manager = initial_tool_manager();
//     let mut messages: Vec<ChatMessage> = vec![
//         ChatMessage::system("你当前工作的目录为 /Users/sean/Desktop/repo/agent-lab。这个目录是你模型的Agent架子，它构建你和外部世界沟通的 bridege。如果你需要什么能力自己修改agent代码补充。"),
//     ];
//     let mut is_auto = false;
//     let mut terminal_line_dirty = false;
   
//     loop {
//         if !is_auto {
//             let mut user_input = String::new();
//             finish_terminal_line(&mut terminal_line_dirty);
//             print!(">");
//             std::io::Write::flush(&mut std::io::stdout())?;
//             if std::io::stdin().read_line(&mut user_input).is_err() {
//                 continue;
//             }
//             if user_input.trim().is_empty() {
//                 continue;
//             }
//             messages.push(ChatMessage::user(user_input));
//         }

//         let mut stream_chat = query_client.stream_chat(&messages, tool_manager.get_tools_scehma());
//         let mut tool_tasks = FuturesUnordered::new();
//         let mut tool_calls = Vec::new();
//         let mut final_assistant_message = String::new();
//         while let Some(model_event)  = stream_chat.next().await {
//             match model_event {
//                 ModelEvent::Text(content) => {
//                     print!("{}", content);
//                     terminal_line_dirty = !content.ends_with('\n');
//                 },
//                 ModelEvent::Thinking(content) => {
//                     print!("\x1b[90m{}\x1b[0m", content);
//                     terminal_line_dirty = !content.ends_with('\n');
//                 },
//                 ModelEvent::ToolCallBlock { id, name, arguments } => {
//                     let tool_call = ToolCall { id, name, arguments };
//                     tool_calls.push(tool_call.clone());
//                     tool_tasks.push(tool_manager.run(tool_call));
//                 },
//                 ModelEvent::Done(assistant_message) => {
//                     final_assistant_message = assistant_message;
//                 }
//                 _ => ()
//             }
//             std::io::Write::flush(&mut std::io::stdout())?;
//         }
//         finish_terminal_line(&mut terminal_line_dirty);

//         let tool_call_ids = tool_calls
//             .iter()
//             .map(|tool_call| tool_call.id.clone())
//             .collect::<Vec<_>>();
//         if tool_calls.len() > 0 {
//             messages.push(ChatMessage::assistant_tool_calls(final_assistant_message, tool_calls));
//             is_auto = true;
//         } else {
//             messages.push(ChatMessage::assistant(final_assistant_message));
//             is_auto = false;
//         }
//         let mut tool_results = Vec::new();
//         while let Some(tool_result) = tool_tasks.next().await {
//             tool_results.push(tool_result);
//         }
//         for tool_call_id in tool_call_ids {
//             if let Some(index) = tool_results
//                 .iter()
//                 .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
//             {
//                 let tool_result = tool_results.remove(index);
//                 messages.push(tool_result);
//             }
//         }
//         for tool_result in tool_results {
//             messages.push(tool_result);
//         }
//     }
// }

// fn finish_terminal_line(terminal_line_dirty: &mut bool) {
//     if *terminal_line_dirty {
//         println!();
//         *terminal_line_dirty = false;
//     }
// }


// fn initial_model() -> anyhow::Result<Box<dyn model::ModelAdapter>> {
//     // 1. 读取环境变量
//     dotenvy::dotenv().ok();

//     let api_key = env::var("DEEPSEEK_API_KEY").unwrap();
//     let deepseek_base_url = env::var("DEEPSEEK_BASE_URL").unwrap();

//     let openai_adapter =
//         model::OpenAiCompatibleAdapter::new(
//             deepseek_base_url,
//             api_key,
//             "deepseek-v4-flash".to_string()
//         );

//     Ok(Box::new(openai_adapter))
// }

// fn initial_tool_manager() -> ToolManager {
//     let mut tool_manager = ToolManager::new();
//     tool_manager.register_tool(Box::new(BashShell));
//     tool_manager
// }
