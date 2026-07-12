use serde::{Deserialize, Serialize};

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;

/// 模型厂商配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// 唯一标识，如 "default-deepseek"。
    pub id: String,
    /// 显示名称，如 "DeepSeek"。
    pub name: String,
    /// provider 标识，如 "DeepSeek" / "OpenAI" / "OpenRouter" / "Custom"。
    pub provider: String,
    /// OpenAI 兼容 API 的 base URL。
    pub base_url: String,
    /// API Key，允许为空，等待用户填写。
    pub api_key: String,
    /// 该 provider 下可用的模型列表。
    pub models: Vec<String>,
}

/// 用户在聊天界面选择的模型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSelection {
    /// 对应 ProviderConfig 的 id。
    pub provider_id: String,
    /// 选中的模型名。
    pub model: String,
}

impl ProviderConfig {
    /// 使用指定模型构造 LLM 客户端，并校验 API Key 与模型可用性。
    pub fn to_llm_with_model(&self, model: &str) -> Result<AgentsLLM, AgentLabError> {
        if self.api_key.is_empty() {
            return Err(AgentLabError::ProviderConfig(format!(
                "Provider [{}] 的 API Key 未填写，请先在设置中填写",
                self.name
            )));
        }
        if !self.models.iter().any(|m| m == model) {
            return Err(AgentLabError::ProviderConfig(format!(
                "Provider [{}] 不支持模型: {}",
                self.name, model
            )));
        }
        Ok(AgentsLLM::from_config_with_model(self, model))
    }
}

impl ModelSelection {
    /// 在 provider 列表中解析出对应的 LLM 客户端。
    pub fn resolve(&self, providers: &[ProviderConfig]) -> Result<AgentsLLM, AgentLabError> {
        let provider = providers
            .iter()
            .find(|p| p.id == self.provider_id)
            .ok_or_else(|| {
                AgentLabError::ProviderConfig(format!("找不到 provider: {}", self.provider_id))
            })?;
        provider.to_llm_with_model(&self.model)
    }
}
