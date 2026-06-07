
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::ToolEvent;

use super::types::{Tool, ToolStream};

// 实现一个 read file 工具，1. 定义工具名称 2. 定义工具描述 3. 定义工具参数 4. 定义工具返回格式
pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    
    fn description(&self) -> &str {
        "It can get file content by filepath"
    }
    
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a text file from the workspace.",
                "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                    "type": "string",
                    "description": "Path to read, relative to the workspace root."
                    },
                    "offset": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Line offset to start reading from."
                    },
                    "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 500,
                    "description": "Maximum number of lines to read."
                    }
                },
                "required": ["path"],
                "additionalProperties": false
                }
            }
        })
    }

fn execute(&self, args: serde_json::Value) -> ToolStream {
    let path = args["path"].as_str().unwrap_or("").to_string();
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(200) as usize;

    let (tx, rx) = mpsc::channel(100);
    let name = self.name().to_string();

    tokio::spawn(async move {
        let tool_event = match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.split('\n').collect();
                let start = offset.min(lines.len());
                let end = offset.saturating_add(limit).min(lines.len());

                ToolEvent::Done(serde_json::json!({
                    "path": path,
                    "offset": offset,
                    "limit": limit,
                    "lines_read": end.saturating_sub(start),
                    "content": lines[start..end].join("\n"),
                }))
            }
            Err(err) => ToolEvent::Err(format!("{} call failed: {}", name, err)),
        };

        let _ = tx.send(tool_event).await;
    });

    Box::pin(ReceiverStream::new(rx))
}
}