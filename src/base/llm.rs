use std::env;

use futures_util::{Stream};
use openai_api_rs::v1::chat_completion::chat_completion_stream::{ChatCompletionStreamRequest, ChatCompletionStreamResponse};
use openai_api_rs::v1::api::OpenAIClient;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Tool};

pub struct AgentsLLM {
    pub model: String,
    pub provider: String,
    base_url: String,
    api_key: String,
    client: OpenAIClient,
}

impl AgentsLLM  {
    fn new(
        model: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        provider: impl Into<String>,
        timeout: impl Into<Option<u64>>,
    ) -> Self {
        let model = model.into();
        let api_key = api_key.into();
        let base_url = base_url.into();
        let provider = provider.into();

        let client = OpenAIClient::builder()
            .with_endpoint(base_url.clone())
            .with_api_key(api_key.clone())
            .with_timeout(timeout.into().unwrap_or(60))
            .build()
            .expect("agent initial failed");

        Self {
            model,
            provider,
            base_url,
            api_key,
            client,
        }
    }

    pub fn get_agents_llm_instance() -> Self {
        // 从环境变量获取模型和提供商
        dotenvy::dotenv().ok();
        let api_key = env::var("API_KEY").expect("API_KEY is not valid");
        let base_url = env::var("BASE_URL").expect("BASE_URL is not valid");
        let model = env::var("MODEL").expect("MODEL is not valid");
        let provider = env::var("PROVIDER").unwrap_or("Custom".into());

        Self::new(model, api_key, base_url, provider, None)
    }

    pub async fn invoke(&self, messages: Vec<ChatCompletionMessage>, tools: Option<Vec<Tool>>, temperature: Option<f64>) -> impl Stream<Item = ChatCompletionStreamResponse> + use<> {
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
