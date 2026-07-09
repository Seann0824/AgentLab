use agent_lab_core::base::agent::AgentStreamEvent;
use agent_lab_core::services::chat_dto::{ChatMessage, SessionSummary};
use tauri::{ipc::Channel, State};

use crate::services::chat;
use crate::state::AppState;

/// 流式聊天命令。
/// 若未提供 session_id，则创建新会话；否则复用已有会话。
#[tauri::command]
pub async fn chat_completion_stream(
    state: State<'_, AppState>,
    channel: Channel<AgentStreamEvent>,
    session_id: Option<String>,
    message: String,
) -> Result<String, String> {
    chat::chat_completion_stream(&state.chat_service, channel, session_id, message).await
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
