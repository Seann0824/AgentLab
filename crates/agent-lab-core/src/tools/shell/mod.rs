use crate::tools::types::{Tool, ToolError};
use std::collections::HashMap;
use tokio::{process::Command, time};

pub struct ShellTool;
impl ShellTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

const DEAFULT_TIMEOUT: u64 = 60 * 1000;

#[async_trait::async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Run a local CLI command."
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let mut properties = HashMap::new();
        properties.insert(
            "command".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: None,
                ..Default::default()
            }),
        );

        openai_api_rs::v1::types::FunctionParameters {
            schema_type: openai_api_rs::v1::types::JSONSchemaType::Object,
            properties: Some(properties),
            required: Some(vec!["command".to_string()]),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let command = args["command"].as_str().unwrap_or("").to_string();

        if command.trim().is_empty() {
            return Err(ToolError::InvalidArgument(
                "command is not empty".to_string(),
            ));
        }

        let mut shell = Command::new("zsh");
        shell.arg("-lc").arg(&command);
        let result = time::timeout(
            std::time::Duration::from_millis(DEAFULT_TIMEOUT),
            shell.output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => Ok(serde_json::json!({
                "command": command,
                "status": output.status.code(),
                "success": output.status.success(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            })
            .to_string()),
            Ok(Err(err)) => Err(ToolError::Internal(format!("command failed: {}", err))),
            Err(_) => Err(ToolError::Internal(format!("command timed out"))),
        }
    }
}
