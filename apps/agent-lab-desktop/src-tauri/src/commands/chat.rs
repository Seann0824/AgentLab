use agent_lab_core::base::agent::AgentStreamEvent;
use agent_lab_core::base::provider_config::ModelSelection;
use agent_lab_core::services::chat_dto::{ChatMessage, SessionSummary};
use tauri::{ipc::Channel, AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::commands::settings::{default_memory_enabled, MEMORY_ENABLED_KEY};
use crate::services::chat;
use crate::state::AppState;

const STORE_NAME: &str = "settings.bin";

/// 流式聊天命令。
/// 若未提供 session_id，则创建新会话；否则复用已有会话。
/// 若提供 model_selection，则由 core ChatService 解析并切换模型；否则使用默认模型。
/// 每次发送时从 store 读取记忆开关，确保设置变更即时生效。
#[tauri::command]
pub async fn chat_completion_stream(
    state: State<'_, AppState>,
    app: AppHandle,
    channel: Channel<AgentStreamEvent>,
    session_id: Option<String>,
    message: String,
    model_selection: Option<ModelSelection>,
) -> Result<String, String> {
    let store = app.store(STORE_NAME).map_err(|e| e.to_string())?;
    let memory_enabled: bool = store
        .get(MEMORY_ENABLED_KEY)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(default_memory_enabled);

    chat::chat_completion_stream(
        &state.chat_service,
        channel,
        session_id,
        message,
        model_selection,
        memory_enabled,
    )
    .await
}

/// 列出所有会话摘要。
#[tauri::command]
pub async fn list_chat_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<SessionSummary>, String> {
    Ok(state.chat_service.list_sessions().await?)
}

/// 获取指定会话的历史消息。
#[tauri::command]
pub async fn get_chat_history(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state
        .chat_service
        .get_session_history(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 创建新会话。
#[tauri::command]
pub async fn create_chat_session(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.chat_service.create_session().await?)
}

/// 删除会话。
#[tauri::command]
pub async fn delete_chat_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    state
        .chat_service
        .delete_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 重命名会话。
#[tauri::command]
pub async fn rename_chat_session(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<bool, String> {
    state
        .chat_service
        .rename_session(&session_id, &title)
        .await
        .map_err(|e| e.to_string())
}
