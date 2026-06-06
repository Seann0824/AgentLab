use std::pin::Pin;

use futures_util::Stream;



pub type ToolStream = Pin<Box<dyn Stream<Item = String> + Send + 'static>>;

pub enum ToolEvent {
    Progress(String),
    Done(String),
    Err,
}

pub trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn execute(&self, args: serde_json::Value) -> ();
}