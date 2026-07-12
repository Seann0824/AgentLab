use agent_lab_core::base::provider_config::{ModelSelection, ProviderConfig};
use serde_json;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_NAME: &str = "settings.bin";
const PROVIDERS_KEY: &str = "providers";
const DEFAULT_MODEL_KEY: &str = "default_model";

fn default_deepseek_provider() -> ProviderConfig {
    ProviderConfig {
        id: "default-deepseek".to_string(),
        name: "DeepSeek".to_string(),
        provider: "DeepSeek".to_string(),
        base_url: "https://api.deepseek.com".to_string(),
        api_key: String::new(),
        models: vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
    }
}

fn default_model_selection() -> ModelSelection {
    ModelSelection {
        provider_id: "default-deepseek".to_string(),
        model: "deepseek-chat".to_string(),
    }
}

/// 读取所有 provider 配置；若为空则自动初始化默认 DeepSeek 配置。
#[tauri::command]
pub async fn list_providers(app: AppHandle) -> Result<Vec<ProviderConfig>, String> {
    let store = app.store(STORE_NAME).map_err(|e| e.to_string())?;
    let configs: Vec<ProviderConfig> = store
        .get(PROVIDERS_KEY)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    if configs.is_empty() {
        let default_configs = vec![default_deepseek_provider()];
        store.set(
            PROVIDERS_KEY,
            serde_json::to_value(&default_configs).map_err(|e| e.to_string())?,
        );
        store.save().map_err(|e| e.to_string())?;
        return Ok(default_configs);
    }

    Ok(configs)
}

/// 新增或更新 provider 配置。
#[tauri::command]
pub async fn save_provider(
    app: AppHandle,
    config: ProviderConfig,
) -> Result<Vec<ProviderConfig>, String> {
    let store = app.store(STORE_NAME).map_err(|e| e.to_string())?;
    let mut configs: Vec<ProviderConfig> = store
        .get(PROVIDERS_KEY)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    if let Some(existing) = configs.iter_mut().find(|c| c.id == config.id) {
        *existing = config;
    } else {
        configs.push(config);
    }

    store.set(
        PROVIDERS_KEY,
        serde_json::to_value(&configs).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(configs)
}

/// 删除 provider 配置。
#[tauri::command]
pub async fn delete_provider(app: AppHandle, id: String) -> Result<Vec<ProviderConfig>, String> {
    let store = app.store(STORE_NAME).map_err(|e| e.to_string())?;
    let mut configs: Vec<ProviderConfig> = store
        .get(PROVIDERS_KEY)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    configs.retain(|c| c.id != id);

    store.set(
        PROVIDERS_KEY,
        serde_json::to_value(&configs).map_err(|e| e.to_string())?,
    );

    // 如果删的是默认模型对应的 provider，重置默认模型。
    let default_model: Option<ModelSelection> = store
        .get(DEFAULT_MODEL_KEY)
        .and_then(|v| serde_json::from_value(v).ok());

    if let Some(dm) = default_model {
        if dm.provider_id == id {
            if let Some(first) = configs.first() {
                if let Some(first_model) = first.models.first() {
                    let new_default = ModelSelection {
                        provider_id: first.id.clone(),
                        model: first_model.clone(),
                    };
                    store.set(
                        DEFAULT_MODEL_KEY,
                        serde_json::to_value(&new_default).map_err(|e| e.to_string())?,
                    );
                }
            } else {
                store.set(
                    DEFAULT_MODEL_KEY,
                    serde_json::to_value::<Option<ModelSelection>>(None)
                        .map_err(|e| e.to_string())?,
                );
            }
        }
    }

    store.save().map_err(|e| e.to_string())?;
    Ok(configs)
}

/// 获取默认模型；若未设置则使用 DeepSeek 默认。
#[tauri::command]
pub async fn get_default_model(app: AppHandle) -> Result<ModelSelection, String> {
    let store = app.store(STORE_NAME).map_err(|e| e.to_string())?;
    let default_model: Option<ModelSelection> = store
        .get(DEFAULT_MODEL_KEY)
        .and_then(|v| serde_json::from_value(v).ok());

    if let Some(dm) = default_model {
        return Ok(dm);
    }

    // 未设置时写入并返回默认 DeepSeek。
    let default = default_model_selection();
    store.set(
        DEFAULT_MODEL_KEY,
        serde_json::to_value(&default).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(default)
}

/// 设置默认模型。
#[tauri::command]
pub async fn set_default_model(
    app: AppHandle,
    selection: ModelSelection,
) -> Result<(), String> {
    let store = app.store(STORE_NAME).map_err(|e| e.to_string())?;
    store.set(
        DEFAULT_MODEL_KEY,
        serde_json::to_value(&selection).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}


