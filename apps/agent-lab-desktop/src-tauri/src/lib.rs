mod commands;
mod services;
mod state;

use agent_lab_core::base::llm::AgentsLLM;
use agent_lab_core::db::get_db_client;
use agent_lab_core::services::{ChatService, MessageService, SessionService};
use agent_lab_core::storage::ChatStore;
use state::AppState;
use tauri::Manager;

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
            let llm = AgentsLLM::from_env().expect("LLM config missing");
            let database_url =
                std::env::var("DATABASE_URL").expect("DATABASE_URL missing");
            let db = tauri::async_runtime::block_on(async {
                get_db_client(&database_url).await
            });
            let chat_store = ChatStore::new(db);
            let session_service = SessionService::new(chat_store.clone());
            let message_service = MessageService::new(chat_store);

            app.manage(AppState {
                chat_service: ChatService::new(
                    llm,
                    session_service,
                    message_service,
                    "default_user",
                ),
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
            commands::chat::list_chat_sessions,
            commands::chat::get_chat_history,
            commands::chat::create_chat_session,
            commands::chat::delete_chat_session,
            commands::chat::rename_chat_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
