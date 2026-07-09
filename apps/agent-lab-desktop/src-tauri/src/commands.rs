#[tauri::command]
#[specta::specta]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
#[specta::specta]
pub fn my_custom_command(invoke_message: String) {
    println!(
        "I was invoked from JavaScript, with this message: {}",
        invoke_message
    );
}

#[tauri::command]
#[specta::specta]
pub fn read_file(file_path: &str) -> Result<Vec<u8>, String> {
    let data = std::fs::read(file_path).map_err(|e| format!("read file failed: {}", e))?;
    Ok(data)
}

#[tauri::command]
#[specta::specta]
pub fn login(user: &str, password: &str) -> Result<String, String> {
    if user == "tauri" && password == "tauri" {
        Ok("logged_in".to_string())
    } else {
        Err("invalid credentials".to_string())
    }
}

use std::sync::Arc;

use agent_lab_core::{
    agent::simple_agent::AgentBuilder,
    base::{agent::Agent, agent::AgentStreamEvent, llm::AgentsLLM},
};
use tauri::{ipc::Channel, State};
use tokio::sync::Mutex;

use crate::state::GlobalState;

#[derive(Clone, serde::Serialize, specta::Type)]
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

#[tauri::command]
pub async fn chat_completion_stream(
    state: State<'_, GlobalState>,
    channel: Channel<AgentStreamEvent>,
    session_id: Option<String>,
    message: String,
) -> Result<String, String> {
    let session_id = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let agent = {
        let mut sessions = state.sessions.write().await;
        sessions
            .entry(session_id.clone())
            .or_insert_with(|| {
                let llm = AgentsLLM::builder()
                    .api_key(std::env::var("API_KEY").unwrap())
                    .base_url(std::env::var("BASE_URL").unwrap())
                    .model(std::env::var("MODEL").unwrap())
                    .provider(std::env::var("PROVIDER").unwrap_or_else(|_| "Custom".into()))
                    .build();
                let agent = AgentBuilder::new().name("test agent").llm(llm).build();

                Arc::new(Mutex::new(agent))
            })
            .clone()
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentStreamEvent>(64);

    // 桥接：内部 tokio channel -> Tauri Channel
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if channel.send(event).is_err() {
                break;
            }
        }
    });

    // 运行 Agent
    tokio::spawn(async move {
        let mut guard = agent.lock().await;
        guard.base_mut().set_event_sender(Some(tx));
        let _ = guard.run(&message).await;
    });

    Ok(session_id)
}
