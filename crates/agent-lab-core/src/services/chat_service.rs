use std::collections::HashMap;
use std::sync::Arc;

use chrono::Local;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

use crate::agent::simple_agent::AgentBuilder;
use crate::agent::Agent;
use crate::base::agent::{Agent as AgentTrait, AgentStreamEvent};
use crate::base::config::Config;
use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::services::chat_dto::{ChatMessage, SessionSummary};

pub type SessionId = String;

/// 会话元数据。
#[derive(Clone)]
struct SessionMeta {
    created_at: i64,
    title: Option<String>,
}

/// 会话存储：session_id -> (Agent 实例, 元数据)。
pub(crate) type Sessions = Arc<RwLock<HashMap<SessionId, (Arc<Mutex<Agent>>, SessionMeta)>>>;

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

    pub(crate) fn with_sessions(sessions: Sessions, llm: AgentsLLM) -> Self {
        Self { sessions, llm }
    }

    fn build_agent(&self) -> Agent {
        AgentBuilder::new()
            .name("chat agent")
            .llm(self.llm.clone())
            .config(Config::default())
            .build()
    }

    /// 创建新会话并返回 session_id。
    pub async fn create_session(&self) -> String {
        let session_id = Uuid::new_v4().to_string();
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            session_id.clone(),
            (
                Arc::new(Mutex::new(self.build_agent())),
                SessionMeta {
                    created_at: Local::now().timestamp(),
                    title: None,
                },
            ),
        );
        session_id
    }

    /// 获取已有会话，或在 session_id 为空时创建新会话。
    pub async fn get_or_create_session(
        &self,
        session_id: Option<String>,
    ) -> Result<(String, Arc<Mutex<Agent>>), AgentLabError> {
        let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let mut sessions = self.sessions.write().await;
        let (agent, _meta) = sessions
            .entry(session_id.clone())
            .or_insert_with(|| {
                (
                    Arc::new(Mutex::new(self.build_agent())),
                    SessionMeta {
                        created_at: Local::now().timestamp(),
                        title: None,
                    },
                )
            })
            .clone();

        Ok((session_id, agent))
    }

    /// 列出所有会话摘要。
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let sessions = self.sessions.read().await;
        let mut summaries = Vec::with_capacity(sessions.len());

        for (id, (agent, meta)) in sessions.iter() {
            let history = agent.lock().await.base().get_history();
            let title = meta.title.clone().unwrap_or_else(|| {
                history
                    .iter()
                    .find(|m| {
                        matches!(
                            m.naive_message.role,
                            openai_api_rs::v1::chat_completion::MessageRole::user
                        )
                    })
                    .map(|m| {
                        let content = match &m.naive_message.content {
                            openai_api_rs::v1::chat_completion::Content::Text(t) => t.clone(),
                            _ => String::new(),
                        };
                        if content.chars().count() > 20 {
                            format!("{}...", content.chars().take(20).collect::<String>())
                        } else {
                            content
                        }
                    })
                    .unwrap_or_else(|| "新会话".to_string())
            });
            let updated_at = history
                .last()
                .map(|m| m.timestamp.timestamp())
                .unwrap_or(meta.created_at);
            summaries.push(SessionSummary {
                id: id.clone(),
                title,
                updated_at,
            });
        }

        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        summaries
    }

    /// 获取指定会话的历史消息。
    pub async fn get_session_history(
        &self,
        session_id: &str,
    ) -> Result<Vec<ChatMessage>, AgentLabError> {
        let sessions = self.sessions.read().await;
        let (agent, _meta) = sessions
            .get(session_id)
            .ok_or_else(|| AgentLabError::InvalidArgument(format!("会话 {} 不存在", session_id)))?;
        let history = agent.lock().await.base().get_history();
        let messages: Vec<ChatMessage> = history
            .iter()
            .filter(|m| !matches!(m.naive_message.role, openai_api_rs::v1::chat_completion::MessageRole::system))
            .map(ChatMessage::from_message)
            .collect();
        Ok(messages)
    }

    /// 删除会话。
    pub async fn delete_session(&self, session_id: &str) -> Result<bool, AgentLabError> {
        let mut sessions = self.sessions.write().await;
        Ok(sessions.remove(session_id).is_some())
    }

    /// 重命名会话。
    pub async fn rename_session(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<bool, AgentLabError> {
        let mut sessions = self.sessions.write().await;
        let Some((_, meta)) = sessions.get_mut(session_id) else {
            return Ok(false);
        };
        meta.title = Some(title.to_string());
        Ok(true)
    }

    /// 在指定会话中运行 Agent，并将流式事件桥接到内部 channel。
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        message: String,
        channel: mpsc::Sender<AgentStreamEvent>,
    ) -> Result<String, AgentLabError> {
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
