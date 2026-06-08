// src/tools/hello_world/mod.rs
//
// HelloWorld — 打印问候信息，用于测试 generate_tool 脚手架生成功能
//
// 此文件由 generate_tool 自动生成
// 生成时间: 2026-06-23 15:06:24

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

pub struct HelloWorld;

impl Tool for HelloWorld {
    fn name(&self) -> &str {
        "hello_world"
    }

    fn description(&self) -> &str {
        "打印问候信息，用于测试 generate_tool 脚手架生成功能"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "hello_world",
                "description": self.description(),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "被问候者的名字"
                        },
                        "greeting": {
                            "type": "string",
                            "description": "自定义问候语，如不提供则使用默认问候"
                        }
                    },
                    "required": ["name"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let _name: String = args["name"].as_str().unwrap_or("").to_string();
            let _greeting: String = args["greeting"].as_str().unwrap_or("").to_string();

            // TODO: 实现工具逻辑
            let result = serde_json::json!({
                "success": true,
                "message": format!("hello_world executed successfully"),
                "note": "This is auto-generated scaffolding. Implement the actual logic here."
            });

            let _ = tx.send(ToolEvent::Done(result)).await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
