use std::pin::Pin;

use futures_util::Stream;



pub type ToolStream = Pin<Box<dyn Stream<Item = ToolEvent> + Send + 'static>>;


#[derive(Debug, Clone)]
pub enum ToolEvent {
    Progress(String),
    Done(serde_json::Value),
    Err(String),
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn execute(&self, args: serde_json::Value) -> ToolStream;
}