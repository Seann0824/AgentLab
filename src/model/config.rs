// src/model/config.rs
//
// ⭐ 模型配置定义 — 描述一个 LLM 提供商的连接信息
//
// ModelConfig 是一个纯数据对象，包含连接一个 LLM 提供商所需的所有参数：
// - name: 唯一标识名（如 "deepseek", "openai-gpt4"）
// - provider: 提供商类型（如 "openai-compatible", "anthropic"）
// - base_url: API 基础 URL
// - api_key: API 密钥
// - model_name: 模型名（如 "deepseek-v4-flash", "gpt-4o"）

/// 模型配置 — 描述一个 LLM 提供商的连接信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelConfig {
    /// 唯一标识名，如 "deepseek", "openai-gpt4", "claude"
    pub name: String,
    /// 提供商类型: "openai-compatible"（目前唯一支持的类型）
    pub provider: String,
    /// API 基础 URL，如 "https://api.deepseek.com"
    pub base_url: String,
    /// API 密钥
    #[serde(skip_serializing)]
    pub api_key: String,
    /// 模型名，如 "deepseek-v4-flash", "gpt-4o"
    pub model_name: String,
}

impl ModelConfig {
    /// 创建新的模型配置
    pub fn new(
        name: impl Into<String>,
        provider: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            provider: provider.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            model_name: model_name.into(),
        }
    }

    /// 显示友好的模型标识字符串，如 "deepseek (deepseek-v4-flash @ api.deepseek.com)"
    pub fn display_name(&self) -> String {
        let host = self
            .base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .split('/')
            .next()
            .unwrap_or(&self.base_url);
        format!("{} ({} @ {})", self.name, self.model_name, host)
    }

    /// 简短的显示名
    pub fn short_name(&self) -> String {
        format!("{}/{}", self.name, self.model_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_config_new() {
        let config = ModelConfig::new(
            "test-model",
            "openai-compatible",
            "https://api.test.com/v1",
            "sk-test-key",
            "test-model-name",
        );
        assert_eq!(config.name, "test-model");
        assert_eq!(config.provider, "openai-compatible");
        assert_eq!(config.base_url, "https://api.test.com/v1");
        assert_eq!(config.api_key, "sk-test-key");
        assert_eq!(config.model_name, "test-model-name");
    }

    #[test]
    fn test_display_name() {
        let config = ModelConfig::new(
            "deepseek",
            "openai-compatible",
            "https://api.deepseek.com",
            "sk-key",
            "deepseek-v4-flash",
        );
        let display = config.display_name();
        assert!(display.contains("deepseek"));
        assert!(display.contains("deepseek-v4-flash"));
        assert!(display.contains("api.deepseek.com"));
    }

    #[test]
    fn test_short_name() {
        let config = ModelConfig::new(
            "my-model",
            "openai-compatible",
            "https://example.com",
            "key",
            "gpt-4o",
        );
        assert_eq!(config.short_name(), "my-model/gpt-4o");
    }
}
