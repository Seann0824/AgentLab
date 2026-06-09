pub mod types;
pub mod openai_compatible;
use futures_util::{Stream};
use openai_api_rs::v1::chat_completion::chat_completion_stream::{ChatCompletionStreamRequest, ChatCompletionStreamResponse};
pub use types::{ChatMessage, ModelEvent, ModelAdapter, ToolCall};

use openai_api_rs::v1::api::OpenAIClient;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Tool};

pub struct AgentLLM {
    model: String,
    client: OpenAIClient,
}

impl AgentLLM  {
    pub fn new(model: impl Into<String>, api_key: &str, base_url: &str, timeout: Option<u64>) -> Self {
        match OpenAIClient::builder()
            .with_endpoint(base_url)
            .with_api_key(api_key)
            .with_timeout(timeout.unwrap_or(60))
            .build() {
                Ok(client) => {
                    return  Self { model: model.into(), client };
                },
                Err(_) => {
                    panic!("agent initial failed");
                }
            };
    }

    pub async fn think(&self, messages: Vec<ChatCompletionMessage>, tools: Option<Vec<Tool>>, temperature: Option<f64>) -> impl Stream<Item = ChatCompletionStreamResponse> {
        // build request
        let req = ChatCompletionStreamRequest::new(
            self.model.clone(),
            messages,
        )
            .temperature(temperature.unwrap_or(0f64))
            .tools(tools.unwrap_or(vec![]))
            .tool_choice(openai_api_rs::v1::chat_completion::ToolChoiceType::Auto);

        let think_stream = self.client.chat_completion_stream(req).await;
        think_stream.unwrap()
    } 
}
