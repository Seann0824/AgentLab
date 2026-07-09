use futures_util::Stream;
use openai_api_rs::v1::api::OpenAIClient;
use openai_api_rs::v1::chat_completion::chat_completion::{
    ChatCompletionRequest, ChatCompletionResponse,
};
use openai_api_rs::v1::chat_completion::chat_completion_stream::{
    ChatCompletionStreamRequest, ChatCompletionStreamResponse,
};
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Tool, ToolChoiceType};

use crate::error::AgentLabError;

pub struct AgentsLLM {
    pub model: String,
    pub provider: String,
    base_url: String,
    api_key: String,
    client: OpenAIClient,
}

impl Clone for AgentsLLM {
    fn clone(&self) -> Self {
        Self::new(
            self.model.clone(),
            self.api_key.clone(),
            self.base_url.clone(),
            self.provider.clone(),
            None::<u64>,
        )
    }
}

impl AgentsLLM {
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

    pub fn builder() -> AgentsLLMBuilder {
        AgentsLLMBuilder::new()
    }

    /// 从环境变量构造 LLM 客户端。
    ///
    /// 读取：
    /// - `API_KEY`（必填）
    /// - `BASE_URL`（必填）
    /// - `MODEL`（必填）
    /// - `PROVIDER`（可选，默认 `Custom`）
    pub fn from_env() -> Result<Self, AgentLabError> {
        let api_key = std::env::var("API_KEY")
            .map_err(|_| AgentLabError::EnvVarMissing { name: "API_KEY" })?;
        let base_url = std::env::var("BASE_URL")
            .map_err(|_| AgentLabError::EnvVarMissing { name: "BASE_URL" })?;
        let model = std::env::var("MODEL")
            .map_err(|_| AgentLabError::EnvVarMissing { name: "MODEL" })?;
        let provider = std::env::var("PROVIDER").unwrap_or_else(|_| "Custom".into());

        Ok(Self::builder()
            .api_key(api_key)
            .base_url(base_url)
            .model(model)
            .provider(provider)
            .build())
    }

    pub async fn invoke(
        &self,
        messages: Vec<ChatCompletionMessage>,
        tools: Option<Vec<Tool>>,
        temperature: Option<f64>,
    ) -> impl Stream<Item = ChatCompletionStreamResponse> + use<> {
        // build request
        let req = ChatCompletionStreamRequest::new(self.model.clone(), messages)
            .temperature(temperature.unwrap_or(0f64))
            .tools(tools.unwrap_or(vec![]))
            .tool_choice(openai_api_rs::v1::chat_completion::ToolChoiceType::Auto);

        let think_stream = self.client.chat_completion_stream(req).await;
        think_stream.unwrap()
    }

    /// 非流式单轮工具调用，适合需要稳定拿到 tool_calls 结果的场景。
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatCompletionMessage>,
        tools: Vec<Tool>,
        tool_choice: ToolChoiceType,
    ) -> Result<ChatCompletionResponse, String> {
        let req = ChatCompletionRequest::new(self.model.clone(), messages)
            .tools(tools)
            .tool_choice(tool_choice)
            .temperature(0.0);

        self.client
            .chat_completion(req)
            .await
            .map(|resp| resp.inner)
            .map_err(|e| format!("[AgentsLLM] chat_completion failed: {}", e))
    }
}

pub struct AgentsLLMBuilder {
    model: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    provider: Option<String>,
    timeout: Option<u64>,
}

impl Default for AgentsLLMBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentsLLMBuilder {
    pub fn new() -> Self {
        Self {
            model: None,
            api_key: None,
            base_url: None,
            provider: None,
            timeout: None,
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    pub fn timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn build(self) -> AgentsLLM {
        let model = self
            .model
            .expect("AgentsLLMBuilder: model is required, use .model(...) to set it");
        let api_key = self
            .api_key
            .expect("AgentsLLMBuilder: api_key is required, use .api_key(...) to set it");
        let base_url = self
            .base_url
            .expect("AgentsLLMBuilder: base_url is required, use .base_url(...) to set it");
        let provider = self.provider.unwrap_or_else(|| "Custom".into());

        AgentsLLM::new(model, api_key, base_url, provider, self.timeout)
    }
}
