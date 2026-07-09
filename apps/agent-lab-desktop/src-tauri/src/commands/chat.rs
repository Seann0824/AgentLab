use agent_lab_core::base::agent::AgentStreamEvent;
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
