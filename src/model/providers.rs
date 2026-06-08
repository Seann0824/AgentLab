// src/model/providers.rs
//
// ⭐ 提供商工厂 — 根据 ModelConfig 构建对应的 ModelAdapter
//
// 当前支持的提供商类型：
// - "openai-compatible": 所有兼容 OpenAI Chat API 格式的提供商

use crate::model::ModelAdapter;
use crate::model::config::ModelConfig;
use crate::model::openai_compatible::OpenAiCompatibleAdapter;

/// 根据 ModelConfig 构建对应的 ModelAdapter
///
/// # 支持的 provider 类型
///
/// | provider 值 | 适配器 | 说明 |
/// |-------------|--------|------|
/// | `openai-compatible` | `OpenAiCompatibleAdapter` | OpenAI 兼容 API（DeepSeek、OpenAI、Groq 等） |
///
/// # 错误
///
/// 如果 provider 类型不支持，返回 Err 包含错误信息。
pub fn build_adapter(config: &ModelConfig) -> Result<Box<dyn ModelAdapter>, String> {
    match config.provider.to_lowercase().as_str() {
        "openai-compatible" | "openai" | "deepseek" => Ok(Box::new(OpenAiCompatibleAdapter::new(
            config.base_url.clone(),
            config.api_key.clone(),
            config.model_name.clone(),
        ))),
        other => Err(format!(
            "不支持的 provider 类型: '{}'。当前支持: openai-compatible",
            other
        )),
    }
}
