use std::env;
use anyhow;
use dotenvy;
use futures_util::{StreamExt, stream::FuturesUnordered};

use crate::{
    context::{ContextManager, ContextStrategy, TokenEstimator},
    model::{ChatMessage, ModelEvent, ToolCall},
    tools::{ToolManager, base_shell::BashShell},
};

mod context;
mod model;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let query_client = initial_model()?;
    let tool_manager = initial_tool_manager();
    let current_dir = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .display()
        .to_string();

    let policy_summary = String::new(); // 权限摘要（后续可从配置加载）

    // ⭐ 定义上下文窗口策略
    let strategy = ContextStrategy::Auto {
        token_limit: 128_000,
        max_turns: 20,
        trigger_ratio: 0.7,
        enable_async_summary: true,
    };
    let token_limit = strategy.token_limit().unwrap_or(128_000);

    // ⭐ 使用 ContextManager 替代 Vec<ChatMessage>
    let mut ctx = ContextManager::new(
        format!(
            "你当前工作的目录为 {}。这个目录是你模型的Agent架子，它构建你和外部世界沟通的 bridge。如果你需要什么能力自己修改agent代码补充。\n\n\
             {} \
             \n\n\
             【上下文管理说明】\n\
             - 为了管理上下文窗口，早期对话历史可能会被自动压缩为摘要。\n\
             - 摘要会按「目标 → 操作 → 决策 → 状态」的结构保留关键信息。\n\
             - 如果发现某些上下文缺失，请基于摘要信息继续工作。\n\
             - 重要的上下文信息请**写入文件**，而不是仅依赖对话历史。\n\
             - 系统状态信息（如 Token 使用率）会输出到 stderr，不会混入你的工具执行结果。\n\
             \n\
             【工作原则】\n\
             - 读取文件内容后，关键信息应记录在文件中，不要仅依赖对话记忆。\n\
             - 如果需要在多轮对话中保持状态，请使用文件持久化。",
            current_dir,
            policy_summary,
        ),
        strategy,
    );

    // ⭐ 启动异步摘要后台任务（可选，需要 ModelAdapter 支持）
    // 如果希望启用 LLM 摘要，传入 Some(query_client.clone())
    // 如果只用规则摘要，传入 None
    ctx.setup_summary_channel(None);

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
            ctx.add_message(ChatMessage::user(user_input));
        }

        // ⭐ 检查是否有异步摘要结果需要注入
        let injected = ctx.poll_summary_results();
        if injected > 0 {
            // 使用 eprint! 输出到 stderr，避免被 Shell 工具捕获
            eprintln!("\r\x1b[2K📋 异步摘要已生成并注入上下文 ({} 条)", injected);
        }

        // ⭐ 显示当前的 Token 使用状态（输出到 stderr）
        let stats = ctx.stats().clone();
        if stats.usage_ratio > 0.3 {
            eprint!(
                "\r\x1b[2K[Token: {}/{} ({:.0}%) | 保留 {} 条重要消息] ",
                TokenEstimator::format_tokens(stats.estimated_tokens),
                TokenEstimator::format_tokens(token_limit),
                stats.usage_ratio * 100.0,
                stats.preserved_count,
            );
        }

        let mut stream_chat = query_client.stream_chat(
            &ctx.get_messages(),
            tool_manager.get_tools_scehma(),
        );
        let mut tool_tasks = FuturesUnordered::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut final_assistant_message = String::new();
        let mut has_tool_calls = false;

        while let Some(model_event) = stream_chat.next().await {
            match model_event {
                ModelEvent::Text(content) => {
                    print!("{}", content);
                    terminal_line_dirty = !content.ends_with('\n');
                }
                ModelEvent::Thinking(content) => {
                    print!("\x1b[90m{}\x1b[0m", content);
                    terminal_line_dirty = !content.ends_with('\n');
                }
                ModelEvent::ToolCallBlock {
                    id,
                    name,
                    arguments,
                } => {
                    finish_terminal_line(&mut terminal_line_dirty);

                    // ===== Tool call visualization =====
                    println!("\x1b[36m━━━ 🔧 调用工具: {}\x1b[0m", name);
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&arguments) {
                        if name == "shell" {
                            if let Some(cmd) = args["command"].as_str() {
                                println!("\x1b[33m  $ {}\x1b[0m", cmd);
                            }
                        } else {
                            println!(
                                "\x1b[33m  {}\x1b[0m",
                                serde_json::to_string_pretty(&args).unwrap_or_default()
                            );
                        }
                    }
                    print!("\x1b[33m⏳ 正在执行...\x1b[0m");
                    std::io::Write::flush(&mut std::io::stdout())?;
                    // ===================================

                    has_tool_calls = true;
                    let tool_call = ToolCall {
                        id,
                        name,
                        arguments,
                    };
                    tool_calls.push(tool_call.clone());
                    tool_tasks.push(tool_manager.run(tool_call));
                }
                ModelEvent::Done(assistant_message) => {
                    final_assistant_message = assistant_message;
                }
                _ => (),
            }
            std::io::Write::flush(&mut std::io::stdout())?;
        }
        finish_terminal_line(&mut terminal_line_dirty);

        let tool_call_ids = tool_calls
            .iter()
            .map(|tool_call| tool_call.id.clone())
            .collect::<Vec<_>>();

        if tool_calls.len() > 0 {
            ctx.add_message(ChatMessage::assistant_tool_calls(
                final_assistant_message,
                tool_calls,
            ));
            is_auto = true;
        } else {
            ctx.add_message(ChatMessage::assistant(final_assistant_message));
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
                render_tool_result_from_msg(tool_result);
            }
        }

        // 将工具结果加入消息
        for tool_call_id in tool_call_ids {
            if let Some(index) = tool_results
                .iter()
                .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
            {
                let tool_result = tool_results.remove(index);

                // ⭐ 如果是关键的工具结果（文件读取等），标记为重要
                let is_important = is_important_tool_result(&tool_result);

                ctx.add_message(tool_result);

                // 对重要工具结果，标记前一条消息（即刚添加的 tool 消息）为 preserved
                if is_important {
                    ctx.preserve_last_message();
                }
            }
        }
        // 剩余的 tool_results（没有对应 tool_call_id 的）
        for tool_result in tool_results {
            ctx.add_message(tool_result);
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
                        println!(
                            "\x1b[32m━━━ ✅ 执行成功 (exit: {}) ━━━\x1b[0m",
                            status.unwrap_or(0)
                        );
                    } else {
                        println!(
                            "\x1b[31m━━━ ❌ 执行失败 (exit: {}) ━━━\x1b[0m",
                            status.unwrap_or(-1)
                        );
                    }
                    if let Some(stdout) = result["stdout"].as_str() {
                        if !stdout.is_empty() {
                            print!("{}", stdout);
                            if !stdout.ends_with('\n') {
                                println!();
                            }
                        }
                    }
                    if let Some(stderr) = result["stderr"].as_str() {
                        if !stderr.is_empty() {
                            print!("\x1b[31m{}\x1b[0m", stderr);
                            if !stderr.ends_with('\n') {
                                println!();
                            }
                        }
                    }
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(result).unwrap_or_default()
                    );
                }
            }
        } else {
            println!("\x1b[31m━━━ ❌ 工具调用失败 ━━━\x1b[0m");
            if let Some(error) = value.get("error") {
                println!(
                    "\x1b[31m  {}\x1b[0m",
                    error["message"].as_str().unwrap_or("unknown error")
                );
            }
        }
    }
}


/// 从 ChatMessage 中提取 content 并渲染工具结果
fn render_tool_result_from_msg(msg: &ChatMessage) {
    if let ChatMessage::Tool { content, .. } = msg {
        render_tool_result(content);
    }
}

/// 判断工具结果是否为重要上下文（文件列表、项目结构等）
fn is_important_tool_result(msg: &ChatMessage) -> bool {
    let ChatMessage::Tool { content, .. } = msg else { return false };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(content) else { return false };
    let Some(stdout) = val
        .get("result")
        .and_then(|r| r.get("stdout"))
        .and_then(|s| s.as_str())
    else { return false };

    context::is_stdout_structural(stdout)
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

    let api_key = env::var("DEEPSEEK_API_KEY")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;
    let deepseek_base_url = env::var("DEEPSEEK_BASE_URL")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_BASE_URL not set"))?;

    let openai_adapter = model::OpenAiCompatibleAdapter::new(
        deepseek_base_url,
        api_key,
        "deepseek-v4-flash".to_string(),
    );

    Ok(Box::new(openai_adapter))
}

fn initial_tool_manager() -> ToolManager {
    let mut tool_manager = ToolManager::new();
    tool_manager.register_tool(Box::new(BashShell));
    tool_manager
}
