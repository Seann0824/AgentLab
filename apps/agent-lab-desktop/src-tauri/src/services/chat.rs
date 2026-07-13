use agent_lab_core::base::agent::AgentStreamEvent;
use agent_lab_core::base::provider_config::ModelSelection;
use agent_lab_core::services::ChatService;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

/// Tauri 适配层：将 core ChatService 的 tokio channel 桥接到 Tauri Channel。
pub async fn chat_completion_stream(
    chat_service: &ChatService,
    channel: Channel<AgentStreamEvent>,
    session_id: Option<String>,
    message: String,
    model_selection: Option<ModelSelection>,
    memory_enabled: bool,
) -> Result<String, String> {
    let (tx, mut rx) = mpsc::channel::<AgentStreamEvent>(64);

    // 桥接：内部 tokio channel -> Tauri Channel
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if channel.send(event).is_err() {
                break;
            }
        }
    });

    chat_service
        .send_message(session_id, message, tx, model_selection, memory_enabled)
        .await
        .map_err(|e| e.to_string())
}
