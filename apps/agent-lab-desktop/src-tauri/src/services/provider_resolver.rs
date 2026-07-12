use agent_lab_core::base::llm::AgentsLLM;
use agent_lab_core::base::provider_config::{ModelSelection, ProviderConfig};
use agent_lab_core::error::AgentLabError;
use agent_lab_core::services::provider_resolver::ProviderResolver;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_NAME: &str = "settings.bin";
const PROVIDERS_KEY: &str = "providers";

/// 从 Tauri store 中读取当前 provider 列表并解析 `ModelSelection`。
///
/// 只负责“读取设置”这一胶水层操作；校验、构造 LLM 等规则交给 core。
pub struct StoreProviderResolver {
    app: AppHandle,
}

impl StoreProviderResolver {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn read_providers(&self) -> Result<Vec<ProviderConfig>, AgentLabError> {
        let store = self
            .app
            .store(STORE_NAME)
            .map_err(|e| AgentLabError::ProviderConfig(format!("读取设置失败: {}", e)))?;
        Ok(store
            .get(PROVIDERS_KEY)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default())
    }
}

impl ProviderResolver for StoreProviderResolver {
    fn resolve(&self, selection: &ModelSelection) -> Result<AgentsLLM, AgentLabError> {
        let providers = self.read_providers()?;
        selection.resolve(&providers)
    }
}
