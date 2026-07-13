pub mod types;
// pub mod base_shell;
pub mod memory;
pub mod memory_tool;
pub mod rag;
pub mod rag_tool;
pub mod time_tool;
pub mod web_search;

use crate::tools::types::{Tool, ToolError};
use futures_util::FutureExt;
use openai_api_rs::v1::chat_completion::{self, ToolCall, ToolType};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;

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
        let schema = self
            .tools
            .values()
            .map(|tool| chat_completion::Tool {
                r#type: ToolType::Function,
                function: openai_api_rs::v1::types::Function {
                    name: tool.name().to_string(),
                    description: Some(tool.description().to_string()),
                    parameters: tool.parameters_schema(),
                },
            })
            .collect();
        schema
    }

    pub async fn run(&self, tool_call: ToolCall) -> (String, String, Result<String, String>) {
        let tool_name = tool_call.function.name.unwrap_or("none".to_string());
        let tool_call_id = tool_call.id;
        let Some(tool) = self.tools.get(&tool_name) else {
            return (
                tool_name.clone(),
                tool_call_id,
                Err(format!("{} 不存在", tool_name)),
            );
        };

        let arguments = tool_call.function.arguments.unwrap_or("{}".to_string());
        let args = serde_json::from_str(&arguments).unwrap_or(serde_json::json!({}));

        // 捕获工具执行期间的 panic（如第三方 crate 内部 unwrap 失败），
        // 将其转换为 ToolError::Internal，避免单个工具拖垮整个 Agent。
        let result = match AssertUnwindSafe(tool.execute(args)).catch_unwind().await {
            Ok(result) => result,
            Err(_) => Err(ToolError::Internal(format!(
                "工具 {} 执行时发生内部崩溃",
                tool_name
            ))),
        };

        // 将结构化错误统一转换为模型可读的文本。
        let result = result.map_err(|e| e.to_agent_message());

        (tool_name, tool_call_id, result)
    }
}
