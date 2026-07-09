mod commands;
mod services;
mod state;

use agent_lab_core::base::llm::AgentsLLM;
use agent_lab_core::services::ChatService;
use state::AppState;
use tauri::Manager;

fn build_llm_from_env() -> Result<AgentsLLM, String> {
    let api_key = std::env::var("API_KEY").map_err(|_| "missing environment variable: API_KEY")?;
    let base_url =
        std::env::var("BASE_URL").map_err(|_| "missing environment variable: BASE_URL")?;
    let model = std::env::var("MODEL").map_err(|_| "missing environment variable: MODEL")?;
    let provider = std::env::var("PROVIDER").unwrap_or_else(|_| "Custom".into());

    Ok(AgentsLLM::builder()
        .api_key(api_key)
        .base_url(base_url)
        .model(model)
        .provider(provider)
        .build())
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
        let specta_builder = tauri_specta::Builder::<tauri::Wry>::new()
            .events(tauri_specta::collect_events![]);
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
            let llm = build_llm_from_env().expect("LLM config missing");
            app.manage(AppState {
                chat_service: ChatService::new(llm),
            });
            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::chat::chat_completion_stream,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
