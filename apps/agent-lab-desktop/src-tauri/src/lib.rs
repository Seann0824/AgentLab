// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
mod state;
use state::GlobalState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(GlobalState {
            name: "test".to_string(),
        })
        .setup(|app| {
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
