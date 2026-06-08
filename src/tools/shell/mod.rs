// src/tools/shell.rs
use tokio::{process::Command, sync::mpsc, time};
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

pub struct BashShell;

const DEAFULT_TIMEOUT: u64 = 30 * 60 * 1000;
impl Tool for BashShell {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Run a local CLI command."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "shell",
                "description": "Run a local CLI command with arguments.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                    },
                    "required": ["command"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let command = args["command"].as_str().unwrap_or("").to_string();
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            if command.trim().is_empty() {
                let _ = tx
                    .send(ToolEvent::Err("command is empty".to_string()))
                    .await;
                return;
            }

            let mut shell = Command::new("zsh");
            shell.arg("-lc").arg(&command);
            let result = time::timeout(
                std::time::Duration::from_millis(DEAFULT_TIMEOUT),
                shell.output(),
            )
            .await;

            let event = match result {
                Ok(Ok(output)) => ToolEvent::Done(serde_json::json!({
                    "command": command,
                    "status": output.status.code(),
                    "success": output.status.success(),
                    "stdout": String::from_utf8_lossy(&output.stdout),
                    "stderr": String::from_utf8_lossy(&output.stderr),
                })),
                Ok(Err(err)) => ToolEvent::Err(format!("command failed: {}", err)),
                Err(_) => ToolEvent::Err("command timed out".to_string()),
            };

            let _ = tx.send(event).await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
