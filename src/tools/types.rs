use std::{collections::HashMap, pin::Pin};

use futures_util::Stream;
use openai_api_rs::v1::{chat_completion, types::JSONSchemaDefine};



pub type ToolStream = Pin<Box<dyn Stream<Item = ToolEvent> + Send + 'static>>;


#[derive(Debug, Clone)]
pub enum ToolEvent {
    Progress(String),
    Done(serde_json::Value),
    Err(String),
}

#[async_trait::async_trait]
pub trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters;
    async fn execute(&self, args: serde_json::Value) -> Result<String, String>;
}