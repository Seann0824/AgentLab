use std::{collections::HashMap, str::FromStr};

use futures_util::StreamExt;

use crate::{model::{ChatMessage, ToolCall}, tools::types::{Tool, ToolEvent}};

pub mod types;
pub mod base_shell;
pub mod edit_tool;
pub mod read_tool;
pub mod search_tool;


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
