use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

use crate::agent::simple_agent::AgentBuilder;
use crate::agent::Agent;
use crate::base::agent::{Agent as AgentTrait, AgentStreamEvent};
use crate::base::config::Config;
use crate::base::llm::AgentsLLM;

pub type SessionId = String;

/// 会话存储：session_id -> Agent 实例。
pub type Sessions = Arc<RwLock<HashMap<SessionId, Arc<Mutex<Agent>>>>>;

/// 聊天业务服务：管理会话生命周期与 Agent 运行。
pub struct ChatService {
    sessions: Sessions,
    llm: AgentsLLM,
}

impl ChatService {
    pub fn new(llm: AgentsLLM) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            llm,
        }
    }

    pub fn with_sessions(sessions: Sessions, llm: AgentsLLM) -> Self {
        Self { sessions, llm }
    }

    /// 获取已有会话，或在 session_id 为空时创建新会话。
    pub async fn get_or_create_session(
        &self,
        session_id: Option<String>,
    ) -> Result<(String, Arc<Mutex<Agent>>), String> {
        let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let mut sessions = self.sessions.write().await;
        let agent = sessions
            .entry(session_id.clone())
            .or_insert_with(|| {
                let agent = AgentBuilder::new()
                    .name("chat agent")
                    .llm(self.llm.clone())
                    .config(Config::default())
                    .build();
                Arc::new(Mutex::new(agent))
            })
            .clone();

        Ok((session_id, agent))
    }

    /// 在指定会话中运行 Agent，并将流式事件桥接到内部 channel。
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        message: String,
        channel: mpsc::Sender<AgentStreamEvent>,
    ) -> Result<String, String> {
        let (session_id, agent) = self.get_or_create_session(session_id).await?;

        // 桥接：内部 tokio channel -> 调用方 channel
        let (tx, mut rx) = mpsc::channel::<AgentStreamEvent>(64);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if channel.send(event).await.is_err() {
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
}
