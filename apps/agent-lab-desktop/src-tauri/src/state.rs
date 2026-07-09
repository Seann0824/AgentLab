use agent_lab_core::Agent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub type SessionId = String;

/// 会话存储：session_id -> Agent 实例。
/// 外层 RwLock 保护 HashMap；内层 Mutex 保护单个 Agent 的可变状态。
pub type Sessions = Arc<RwLock<HashMap<SessionId, Arc<Mutex<Agent>>>>>;

/// 全局应用状态
pub struct AppState {
    pub sessions: Sessions,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
