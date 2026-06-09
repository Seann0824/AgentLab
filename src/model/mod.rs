pub mod types;
pub mod openai_compatible;

use futures_util::StreamExt;
use openai_api_rs::v1::chat_completion::chat_completion_stream::{ChatCompletionStreamRequest, ChatCompletionStreamResponse};
pub use types::{ChatMessage, ModelEvent, ModelAdapter, ToolCall};
pub use openai_compatible::OpenAiCompatibleAdapter;

use openai_api_rs::v1::api::OpenAIClient;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage};

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

    pub async fn think(&self, messages: Vec<ChatCompletionMessage>, temperature: Option<f64>) -> Result<String, String> {
        // build request
        let req = ChatCompletionStreamRequest::new(
            self.model.clone(),
            messages,
        ).temperature(temperature.unwrap_or(0f64));
        let mut collected_content = vec![];
        match self.client.chat_completion_stream(req).await {
            Ok(mut stream) => {
                while let Some(chunck) = stream.next().await {
                    match chunck {
                        ChatCompletionStreamResponse::Content(content) => {
                            collected_content.push(content);
                        },
                        ChatCompletionStreamResponse::Reasoning(String) => {

                        },
                        ChatCompletionStreamResponse::ToolCall(tools_call) => {

                        },
                        ChatCompletionStreamResponse::Done=> (),
                    }
                }
            },
            Err(_) => return Err("error".into()),
        }
        Ok(collected_content.join(""))
    } 
}
