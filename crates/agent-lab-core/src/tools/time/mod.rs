use chrono::{Local, Utc};
use openai_api_rs::v1::types;
use std::collections::HashMap;

use crate::tools::types::{Tool, ToolError};

/// 获取当前时间的工具。
///
/// 让 Agent 能够获取运行环境的真实当前时间，而不是依赖训练数据中的知识截止时间。
pub struct TimeTool;

impl TimeTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TimeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for TimeTool {
    fn name(&self) -> &str {
        "get_current_time"
    }

    fn description(&self) -> &str {
        "获取当前时间。当用户询问时间、日期、当前时刻，或任何依赖实时时间的问题时，必须调用此工具获取准确时间，不要依赖训练知识。"
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let properties = HashMap::from([(
            "format".to_string(),
            Box::new(types::JSONSchemaDefine {
                schema_type: Some(types::JSONSchemaType::String),
                description: Some(
                    "时间格式，可选 'iso'（ISO 8601）、'readable'（可读格式）。默认为 'readable'。"
                        .to_string(),
                ),
                ..Default::default()
            }),
        )]);

        openai_api_rs::v1::types::FunctionParameters {
            schema_type: types::JSONSchemaType::Object,
            properties: Some(properties),
            required: None,
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let format = args["format"].as_str().unwrap_or("readable");
        let local = Local::now();
        let utc = Utc::now();

        if format.eq_ignore_ascii_case("iso") {
            Ok(format!(
                "本地时间：{}\nUTC 时间：{}",
                local.to_rfc3339(),
                utc.to_rfc3339()
            ))
        } else {
            Ok(format!(
                "当前本地时间：{}\n当前 UTC 时间：{}\n时间戳（秒）：{}",
                local.format("%Y-%m-%d %H:%M:%S %:z"),
                utc.format("%Y-%m-%d %H:%M:%S UTC"),
                local.timestamp()
            ))
        }
    }
}
