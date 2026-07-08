// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
mod state;
use state::GlobalState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
            app.manage(GlobalState {
                name: "test".to_string(),
            });
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::my_custom_command,
            commands::read_file,
            commands::login,
            commands::read_file_channel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
