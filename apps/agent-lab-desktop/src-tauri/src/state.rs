use agent_lab_core::services::{ChatService, RagService};

/// 全局应用状态
pub struct AppState {
    pub chat_service: ChatService,
    pub rag_service: RagService,
}
