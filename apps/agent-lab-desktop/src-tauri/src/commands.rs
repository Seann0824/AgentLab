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
pub fn read_file(file_path: &str) -> Result<Response, String> {
    let data = std::fs::read(file_path).map_err(|e| format!("read file failed: {}", e))?;
    Ok(tauri::ipc::Response::new(data))
}

#[tauri::command]
pub fn login(user: &str, password: &str) -> Result<String, String> {
    if user == "tauri" && password == "tauri" {
        Ok("logged_in".to_string())
    } else {
        Err("invalid credentials".to_string())
    }
}

use tauri::ipc::Channel;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChunk {
    pub chunk: Vec<u8>,
    pub progress: f64,
    pub done: bool,
}

#[tauri::command]
pub async fn read_file_channel(
    file_path: String,
    on_chunk: Channel<FileChunk>,
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(file_path)
        .await
        .map_err(|e| format!("open file failed: {}", e))?;
    let total_size = file
        .metadata()
        .await
        .map_err(|e| format!("get metadata failed: {}", e))?
        .len() as f64;

    let mut buffer = vec![0; 4096];
    let mut read = 0usize;

    loop {
        let len = file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("read file failed: {}", e))?;
        if len == 0 {
            on_chunk
                .send(FileChunk {
                    chunk: vec![],
                    progress: 1.0,
                    done: true,
                })
                .map_err(|e| format!("send chunk failed: {}", e))?;
            break;
        }

        read += len;
        on_chunk
            .send(FileChunk {
                chunk: buffer[..len].to_vec(),
                progress: if total_size > 0.0 {
                    read as f64 / total_size
                } else {
                    0.0
                },
                done: false,
            })
            .map_err(|e| format!("send chunk failed: {}", e))?;
    }

    Ok(())
}
