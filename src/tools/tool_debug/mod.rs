// src/tools/debug_tool/mod.rs
//
// DebugTool — 允许 Agent 在运行时读取/设置全局 debug 标志
//
// 功能：
//   - `action: "status"` — 返回当前 debug 状态
//   - `action: "enable"` — 开启 debug 模式
//   - `action: "disable"` — 关闭 debug 模式
//   - `action: "toggle"` — 切换 debug 模式

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

pub struct DebugTool;

impl Tool for DebugTool {
    fn name(&self) -> &str {
        "debug"
    }

    fn description(&self) -> &str {
        "读取或设置全局 debug 模式。当 debug 开启时，代码中所有 debug 条件判断的代码块都会执行（输出调试日志等）。支持 action: status|enable|disable|toggle"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "debug",
                "description": "读取或设置全局 debug 模式。当 debug 开启时，代码中所有 debug 条件判断的代码块都会执行。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "要执行的操作：status（查看状态）、enable（开启）、disable（关闭）、toggle（切换）",
                            "enum": ["status", "enable", "disable", "toggle"]
                        }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let action = args["action"].as_str().unwrap_or("status");

            let result = match action {
                "enable" => {
                    crate::debug::enable();
                    serde_json::json!({
                        "success": true,
                        "previous_state": false,
                        "current_state": true,
                        "message": "debug 模式已开启"
                    })
                }
                "disable" => {
                    let was_enabled = crate::debug::is_enabled();
                    crate::debug::disable();
                    serde_json::json!({
                        "success": true,
                        "previous_state": was_enabled,
                        "current_state": false,
                        "message": "debug 模式已关闭"
                    })
                }
                "toggle" => {
                    let new_state = crate::debug::toggle();
                    if new_state {
                        serde_json::json!({
                            "success": true,
                            "previous_state": !new_state,
                            "current_state": new_state,
                            "message": "debug 模式已切换为开启"
                        })
                    } else {
                        serde_json::json!({
                            "success": true,
                            "previous_state": !new_state,
                            "current_state": new_state,
                            "message": "debug 模式已切换为关闭"
                        })
                    }
                }
                _ => {
                    // "status" 或未知操作
                    let enabled = crate::debug::is_enabled();
                    serde_json::json!({
                        "success": true,
                        "enabled": enabled,
                        "status_text": crate::debug::status_text(),
                    })
                }
            };

            let _ = tx.send(ToolEvent::Done(result)).await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
