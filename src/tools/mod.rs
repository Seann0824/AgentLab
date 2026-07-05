pub mod types;
// pub mod base_shell;
pub mod web_search;
pub mod memory;

use std::{collections::HashMap};
use openai_api_rs::v1::chat_completion::{self, ToolCall, ToolType};
use crate::{tools::types::Tool};

pub struct ToolManager {
    tools: HashMap<String, Box<dyn Tool + Send + Sync>>,
}

impl ToolManager {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register_tool(&mut self, tool: Box<dyn Tool + Send + Sync>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn with_tool(mut self, tool: Box<dyn Tool + Send + Sync>) -> Self {
        self.tools.insert(tool.name().to_string(), tool);
        self
    }

    pub fn remove_tool(&mut self, tool_name: &String) {
        self.tools.remove(tool_name);
    }

    pub fn get_tools_scehma(&self) -> Vec<chat_completion::Tool> {
        let schema = self.tools
            .values()
            .map(|tool| {
                chat_completion::Tool {
                    r#type: ToolType::Function,
                    function: openai_api_rs::v1::types::Function {
                        name: tool.name().to_string(),
                        description: Some(tool.description().to_string()),
                        parameters: tool.parameters_schema()
                    },
                }
            })
            .collect();
        schema
    }

    pub async fn run(&self, tool_call: ToolCall) -> (String, Result<String, String>) {
        let tool_name = tool_call.function.name.unwrap_or("none".to_string());
        let tool_call_id = tool_call.id;
        let Some(tool) = self.tools.get(&tool_name) else {
            return (tool_call_id, Err(format!("{} 不存在", tool_name)));
        };

        let arguments = tool_call.function.arguments.unwrap_or("{}".to_string());
        (tool_call_id, tool.execute(serde_json::from_str(&arguments).unwrap_or(serde_json::json!({}))).await)
    }
}
