use std::pin::Pin;

use futures_util::Stream;

pub type ToolStream = Pin<Box<dyn Stream<Item = ToolEvent> + Send + 'static>>;

#[derive(Debug, Clone)]
pub enum ToolEvent {
    Progress(String),
    Done(serde_json::Value),
    Err(String),
}

/// 工具契约快照 — 可用于能力发现、审计和自我迭代前后的对比。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolContract {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn execute(&self, args: serde_json::Value) -> ToolStream;
}
