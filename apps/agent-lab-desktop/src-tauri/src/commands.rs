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
