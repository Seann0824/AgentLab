#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub fn my_custom_command(invoke_message: String) {
    println!(
        "I was invoked from JavaScript, with this message: {}",
        invoke_message
    );
}

use tauri::ipc::Response;
#[tauri::command]
pub fn read_file(file_path: &str) -> Response {
    let data = std::fs::read(file_path).unwrap();
    tauri::ipc::Response::new(data)
}

#[tauri::command]
pub fn login(user: &str, password: &str) -> Result<String, String> {
    if user == "tauri" && password == "tauri" {
        Ok("logged_in".to_string())
    } else {
        Err("invalid credentials".to_string())
    }
}
