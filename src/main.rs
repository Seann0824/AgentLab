use std::env;
use anyhow;
use dotenvy;
use futures_util::{StreamExt, stream::FuturesUnordered};

use crate::{model::{ChatMessage, ModelEvent, ToolCall}, tools::{ToolManager, base_shell::BashShell}};

mod model;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let query_client = initial_model()?;
    let tool_manager = initial_tool_manager();
    let mut messages: Vec<ChatMessage> = vec![
        ChatMessage::system("你当前工作的目录为 /Users/sean/Desktop/repo/agent-lab。这个目录是你模型的Agent架子，它构建你和外部世界沟通的 bridege。如果你需要什么能力自己修改agent代码补充。"),
    ];
    let mut is_auto = false;
    let mut terminal_line_dirty = false;
   
    loop {
        if !is_auto {
            let mut user_input = String::new();
            finish_terminal_line(&mut terminal_line_dirty);
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
        let mut final_assistant_message = String::new();
        let mut has_tool_calls = false;
        while let Some(model_event)  = stream_chat.next().await {
            match model_event {
                ModelEvent::Text(content) => {
                    print!("{}", content);
                    terminal_line_dirty = !content.ends_with('\n');
                },
                ModelEvent::Thinking(content) => {
                    print!("\x1b[90m{}\x1b[0m", content);
                    terminal_line_dirty = !content.ends_with('\n');
                },
                ModelEvent::ToolCallBlock { id, name, arguments } => {
                    finish_terminal_line(&mut terminal_line_dirty);

                    // ===== Tool call visualization =====
                    println!("\x1b[36m━━━ 🔧 调用工具: {}\x1b[0m", name);
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&arguments) {
                        if name == "shell" {
                            if let Some(cmd) = args["command"].as_str() {
                                println!("\x1b[33m  $ {}\x1b[0m", cmd);
                            }
                        } else {
                            println!("\x1b[33m  {}\x1b[0m",
                                serde_json::to_string_pretty(&args).unwrap_or_default());
                        }
                    }
                    print!("\x1b[33m⏳ 正在执行...\x1b[0m");
                    std::io::Write::flush(&mut std::io::stdout())?;
                    // ===================================

                    has_tool_calls = true;
                    let tool_call = ToolCall { id, name, arguments };
                    tool_calls.push(tool_call.clone());
                    tool_tasks.push(tool_manager.run(tool_call));
                },
                ModelEvent::Done(assistant_message) => {
                    final_assistant_message = assistant_message;
                }
                _ => ()
            }
            std::io::Write::flush(&mut std::io::stdout())?;
        }
        finish_terminal_line(&mut terminal_line_dirty);

        let tool_call_ids = tool_calls
            .iter()
            .map(|tool_call| tool_call.id.clone())
            .collect::<Vec<_>>();
        if tool_calls.len() > 0 {
            messages.push(ChatMessage::assistant_tool_calls(final_assistant_message, tool_calls));
            is_auto = true;
        } else {
            messages.push(ChatMessage::assistant(final_assistant_message));
            is_auto = false;
        }
        let mut tool_results = Vec::new();
        while let Some(tool_result) = tool_tasks.next().await {
            tool_results.push(tool_result);
        }

        // Clear loading line and render tool results
        if has_tool_calls {
            print!("\r\x1b[K");
            for tool_result in &tool_results {
                if let ChatMessage::Tool { content, .. } = tool_result {
                    render_tool_result(content);
                }
            }
        }

        for tool_call_id in tool_call_ids {
            if let Some(index) = tool_results
                .iter()
                .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
            {
                let tool_result = tool_results.remove(index);
                messages.push(tool_result);
            }
        }
        for tool_result in tool_results {
            messages.push(tool_result);
        }
    }
}

fn render_tool_result(content: &str) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        let ok = value["ok"].as_bool().unwrap_or(false);
        if ok {
            if let Some(result) = value.get("result") {
                if result.is_object() {
                    let success = result["success"].as_bool().unwrap_or(true);
                    let status = result["status"].as_i64();
                    if success {
                        println!("\x1b[32m━━━ ✅ 执行成功 (exit: {}) ━━━\x1b[0m", status.unwrap_or(0));
                    } else {
                        println!("\x1b[31m━━━ ❌ 执行失败 (exit: {}) ━━━\x1b[0m", status.unwrap_or(-1));
                    }
                    if let Some(stdout) = result["stdout"].as_str() {
                        if !stdout.is_empty() {
                            print!("{}", stdout);
                            if !stdout.ends_with('\n') { println!(); }
                        }
                    }
                    if let Some(stderr) = result["stderr"].as_str() {
                        if !stderr.is_empty() {
                            print!("\x1b[31m{}\x1b[0m", stderr);
                            if !stderr.ends_with('\n') { println!(); }
                        }
                    }
                } else {
                    println!("{}", serde_json::to_string_pretty(result).unwrap_or_default());
                }
            }
        } else {
            println!("\x1b[31m━━━ ❌ 工具调用失败 ━━━\x1b[0m");
            if let Some(error) = value.get("error") {
                println!("\x1b[31m  {}\x1b[0m", error["message"].as_str().unwrap_or("unknown error"));
            }
        }
    }
}

fn finish_terminal_line(terminal_line_dirty: &mut bool) {
    if *terminal_line_dirty {
        println!();
        *terminal_line_dirty = false;
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
    tool_manager.register_tool(Box::new(BashShell));
    tool_manager
}
