use crate::base::llm::AgentsLLM;
use crate::base::provider_config::ModelSelection;
use crate::error::AgentLabError;

/// Provider 解析器：根据 `ModelSelection` 返回对应的 `AgentsLLM`。
///
/// 由上层（如桌面端）实现，负责提供当前可用的 provider 配置；
/// core 只关心解析与校验规则，不依赖具体存储实现。
pub trait ProviderResolver: Send + Sync {
    fn resolve(&self, selection: &ModelSelection) -> Result<AgentsLLM, AgentLabError>;
}
