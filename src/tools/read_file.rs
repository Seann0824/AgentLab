use std::fs;

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

    fn execute(&self, args: serde_json::Value) -> () {
        let path = args["path"].as_str().unwrap_or("");
        let offset = args["offset"].as_u64().unwrap_or(0);
        let limit = args["limit"].as_u64().unwrap_or(200);
        // 1. 找到文件 2. 打开 3. 获得对应的行数据
        ()
    }
}