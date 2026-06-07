use std::collections::HashMap;
use std::str::FromStr;

use futures_util::StreamExt;

use crate::model::{ChatMessage, ToolCall};
use crate::tools::types::{Tool, ToolEvent};

/// 工具摘要信息，用于动态生成系统提示词和交互式工具列表
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

pub mod types;
pub mod shell;
pub mod tool_debug;
pub mod edit;
pub mod read;
pub mod search;
pub mod subagent;
pub mod investigate;


pub struct ToolManager {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolManager {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get_tools_scehma(&self) -> serde_json::Value {
        let tools_schema = self.tools
            .values()
            .map(|tool| {
                tool.parameters_schema()
            })
            .collect::<Vec<serde_json::Value>>();
        serde_json::json!(tools_schema)
    }

    /// 返回所有已注册工具的摘要信息列表，用于动态生成系统提示词和 `/tools` 命令
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        let mut tools: Vec<ToolInfo> = self.tools
            .values()
            .map(|tool| ToolInfo {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
            })
            .collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    pub async fn run(&self, tool_call: ToolCall) -> ChatMessage {
        let id = tool_call.id;
        let name = tool_call.name;
        let arguments = tool_call.arguments;

        match self.tools.get(&name) {
            Some(tool) => {
                let Ok(args) = serde_json::Value::from_str(&arguments) else {
                    let content = serde_json::json!({
                        "ok": false,
                        "error": {
                            "code": "invalid_arguments",
                            "message": format!("{} arguments are not valid JSON: {}", name, arguments),
                        },
                    });

                    return tool_message(&id, content);
                };
                let mut result_stream = tool.execute(args);
                while let Some(tool_event) = result_stream.next().await {
                    match tool_event {
                        ToolEvent::Done(result) => {
                            let content = serde_json::json!({
                                "ok": true,
                                "result": result,
                            });

                            return tool_message(&id, content);
                        }
                        ToolEvent::Err(message) => {
                            let content = serde_json::json!({
                                "ok": false,
                                "error": {
                                    "code": "tool_failed",
                                    "message": message,
                                },
                            });

                            return tool_message(&id, content);
                        }

                        _ => (),
                    }
                }

                let content = serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": "tool_no_result",
                        "message": format!("{} did not return a result", name),
                    },
                });

                tool_message(&id, content)
            },
            _ => {
                let content = serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": "unknown_tool",
                        "message": format!("unknown tool: {}", name),
                    },
                });

                tool_message(&id, content)
            },
        }
    }
}

fn tool_message(id: &str, content: serde_json::Value) -> ChatMessage {
    ChatMessage::tool(id.to_string(), content.to_string())
}
