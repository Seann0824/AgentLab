use agent_lab_core::Agent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub type SessionId = String;

pub type Sessions = Arc<RwLock<HashMap<SessionId, Arc<Mutex<Agent>>>>>;
/**
 * 多 Agent， 那么我们要用 sessionId 来管理多Agent场景。
 * arc 让多个能持有 hashMap
 * 但是我们获取 hashmap 里面的Agent 实例子，是否需要通过 Arc ? 不太懂，一个HashMap 是否同时支持多个线程访问，每个线程访问的是不同的sessionId 对应的Agent
 */
pub struct GlobalState {
    pub name: String,
    pub sessions: Sessions,
}
