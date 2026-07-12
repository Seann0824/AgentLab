mod commands;
mod services;
mod state;

use agent_lab_core::base::llm::AgentsLLM;
use agent_lab_core::base::provider_config::{ModelSelection, ProviderConfig};
use agent_lab_core::db::get_db_client;
use agent_lab_core::services::{ChatService, MessageService, RagService, SessionService};
use agent_lab_core::storage::ChatStore;
use agent_lab_core::tools::memory_tool::MemoryTool;
use state::AppState;
use tauri::Manager;
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

/// 从 store 读取 providers；为空时写入默认 DeepSeek 配置。
fn init_providers(app: &tauri::AppHandle) -> Result<Vec<ProviderConfig>, String> {
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

/// 从 store 读取默认模型；未设置时写入 DeepSeek 默认。
fn init_default_model(app: &tauri::AppHandle) -> Result<ModelSelection, String> {
    let store = app.store(STORE_NAME).map_err(|e| e.to_string())?;
    let default_model: Option<ModelSelection> = store
        .get(DEFAULT_MODEL_KEY)
        .and_then(|v| serde_json::from_value(v).ok());

    if let Some(dm) = default_model {
        return Ok(dm);
    }

    let default = default_model_selection();
    store.set(
        DEFAULT_MODEL_KEY,
        serde_json::to_value(&default).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(default)
}

/// 根据默认模型配置构造 LLM。
/// 若默认 provider 存在，即使 api_key 为空也使用它（启动时不强依赖 key）。
fn build_default_llm(
    providers: &[ProviderConfig],
    default_model: &ModelSelection,
) -> Result<AgentsLLM, String> {
    let provider = providers
        .iter()
        .find(|p| p.id == default_model.provider_id)
        .ok_or_else(|| format!("找不到默认 provider: {}", default_model.provider_id))?;

    let model = if provider.models.contains(&default_model.model) {
        default_model.model.clone()
    } else {
        provider.models.first().cloned().unwrap_or_default()
    };
    Ok(AgentsLLM::from_config_with_model(provider, &model))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 从当前目录向上查找并加载 .env
    dotenvy::dotenv().ok();

    // tauri-specta：编译时生成 TypeScript bindings
    // 当前 chat_completion_stream 使用 agent-lab-core 的 AgentStreamEvent，
    // 该类型尚未 derive specta::Type，因此暂不纳入 specta 收集。
    // 前端直接通过 invoke 调用本命令。
    #[cfg(debug_assertions)]
    {
        let specta_builder =
            tauri_specta::Builder::<tauri::Wry>::new().events(tauri_specta::collect_events![]);
        specta_builder
            .export(
                specta_typescript::Typescript::default(),
                "../src/bindings.ts",
            )
            .expect("failed to export typescript bindings");
    }

    // CrabNebula DevTools：只在 debug 构建中启用，用于实时查看日志、command 性能等
    #[cfg(debug_assertions)]
    let devtools = tauri_plugin_devtools::init();

    let mut builder = tauri::Builder::default();

    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(devtools);
    }

    builder
        .setup(|app| {
            let providers = init_providers(&app.handle())?;
            let default_model = init_default_model(&app.handle())?;
            let llm = build_default_llm(&providers, &default_model)
                .map_err(|e| format!("构建默认 LLM 失败: {}", e))?;

            let database_url =
                std::env::var("DATABASE_URL").expect("DATABASE_URL missing");
            let neo4j_url = std::env::var("NEO4J_URL").unwrap_or_else(|_| "neo4j://127.0.0.1:7687".into());
            let neo4j_user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
            let neo4j_password = std::env::var("NEO4J_PASSWORD").unwrap_or_default();

            let db = tauri::async_runtime::block_on(async { get_db_client(&database_url).await });
            let chat_store = ChatStore::new(db.clone());
            let session_service = SessionService::new(chat_store.clone());
            let message_service = MessageService::new(chat_store);
            let rag_service = RagService::with_default_embedder(db, llm.clone());
            let resolver = crate::services::provider_resolver::StoreProviderResolver::new(
                app.handle().clone(),
            );

            let memory_tool = tauri::async_runtime::block_on(async {
                if neo4j_password.is_empty() {
                    eprintln!("NEO4J_PASSWORD 未设置，memory tool 不可用");
                    return None;
                }
                match MemoryTool::new(
                    llm.clone(),
                    &database_url,
                    neo4j_url,
                    neo4j_user,
                    neo4j_password,
                )
                .await
                {
                    Ok(tool) => Some(tool),
                    Err(e) => {
                        eprintln!("MemoryTool 初始化失败，memory tool 不可用: {}", e);
                        None
                    }
                }
            });

            let mut chat_service = ChatService::new(
                llm,
                session_service,
                message_service,
                "default_user",
            )
            .with_resolver(resolver)
            .with_rag_service(rag_service.clone());
            if let Some(tool) = memory_tool {
                chat_service = chat_service.with_memory_tool(tool);
            }

            app.manage(AppState {
                chat_service,
                rag_service,
            });
            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::chat::chat_completion_stream,
            commands::chat::list_chat_sessions,
            commands::chat::get_chat_history,
            commands::chat::create_chat_session,
            commands::chat::delete_chat_session,
            commands::chat::rename_chat_session,
            commands::rag::index_document_content,
            commands::rag::list_namespaces,
            commands::rag::delete_namespace,
            commands::settings::list_providers,
            commands::settings::save_provider,
            commands::settings::delete_provider,
            commands::settings::get_default_model,
            commands::settings::set_default_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
