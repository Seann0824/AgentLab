use agent_lab_core::base::agent::AgentStreamEvent;
use tauri::{ipc::Channel, State};

use crate::services::ChatService;
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
    let chat_service = ChatService::new(state.sessions.clone());

    let (session_id, agent) = chat_service
        .get_or_create_session(session_id)
        .await
        .map_err(String::from)?;

    chat_service
        .run_agent(agent, message, channel)
        .await
        .map_err(String::from)?;

    Ok(session_id)
}
