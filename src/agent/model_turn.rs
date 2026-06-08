use futures_util::{StreamExt, stream::FuturesUnordered};

use super::Agent;
use super::render::finish_terminal_line;
use crate::model::{ChatMessage, ModelEvent, ToolCall};

pub(super) struct ModelTurn {
    pub(super) final_assistant_message: String,
    pub(super) tool_calls: Vec<ToolCall>,
    pub(super) tool_call_ids: Vec<String>,
    pub(super) tool_results: Vec<ChatMessage>,
    pub(super) has_tool_calls: bool,
}

impl Agent {
    pub(super) async fn run_model_turn(
        &mut self,
        terminal_line_dirty: &mut bool,
    ) -> anyhow::Result<ModelTurn> {
        let current_adapter = self
            .model_manager
            .current_adapter()
            .expect("当前没有可用的模型适配器");
        let mut stream_chat = current_adapter.stream_chat(
            &self.context_manager.get_messages(),
            self.tool_manager.get_tools_scehma(),
        );
        let mut tool_tasks = FuturesUnordered::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut final_assistant_message = String::new();
        let mut has_tool_calls = false;

        while let Some(model_event) = stream_chat.next().await {
            match model_event {
                ModelEvent::Text(content) => {
                    print!("{}", content);
                    *terminal_line_dirty = !content.ends_with('\n');
                }
                ModelEvent::Thinking(content) => {
                    print!("\x1b[90m{}\x1b[0m", content);
                    *terminal_line_dirty = !content.ends_with('\n');
                }
                ModelEvent::ToolCallBlock {
                    id,
                    name,
                    arguments,
                } => {
                    finish_terminal_line(terminal_line_dirty);
                    render_tool_call(&name, &arguments)?;
                    has_tool_calls = true;
                    let tool_call = ToolCall {
                        id,
                        name,
                        arguments,
                    };
                    tool_calls.push(tool_call.clone());
                    tool_tasks.push(self.tool_manager.run(tool_call));
                }
                ModelEvent::Done(assistant_message) => {
                    final_assistant_message = assistant_message;
                }
                ModelEvent::Error(err) => {
                    eprintln!("\r\x1b[2K\x1b[31m❌ 模型 API 错误: {}\x1b[0m", err);
                }
            }
            std::io::Write::flush(&mut std::io::stdout())?;
        }
        finish_terminal_line(terminal_line_dirty);

        let tool_call_ids = tool_calls
            .iter()
            .map(|tool_call| tool_call.id.clone())
            .collect();
        let mut tool_results = Vec::new();
        while let Some(tool_result) = tool_tasks.next().await {
            tool_results.push(tool_result);
        }

        Ok(ModelTurn {
            final_assistant_message,
            tool_calls,
            tool_call_ids,
            tool_results,
            has_tool_calls,
        })
    }
}

fn render_tool_call(name: &str, arguments: &str) -> anyhow::Result<()> {
    println!("\x1b[36m━━━ 🔧 调用工具: {}\x1b[0m", name);
    if let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments) {
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
    Ok(())
}
