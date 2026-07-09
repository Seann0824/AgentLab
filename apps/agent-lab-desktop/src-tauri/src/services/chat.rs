use std::sync::Arc;

use agent_lab_core::{
    agent::simple_agent::AgentBuilder,
    base::{
        agent::{Agent as AgentTrait, AgentStreamEvent},
        config::Config,
        llm::AgentsLLM,
    },
    Agent,
};
use tauri::ipc::Channel;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::Sessions;

/// 聊天业务服务：管理会话生命周期与 Agent 运行。
pub struct ChatService {
    sessions: Sessions,
}

impl ChatService {
    pub fn new(sessions: Sessions) -> Self {
        Self { sessions }
    }

    /// 获取已有会话，或在 session_id 为空时创建新会话。
    pub async fn get_or_create_session(
        &self,
        session_id: Option<String>,
    ) -> Result<(String, Arc<Mutex<Agent>>), AppError> {
        let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        // 先构建 LLM；若环境变量缺失则提前返回，不会污染会话表。
        let llm = build_llm_from_env()?;

        let mut sessions = self.sessions.write().await;
        let agent = sessions
            .entry(session_id.clone())
            .or_insert_with(|| {
                let agent = AgentBuilder::new()
                    .name("desktop agent")
                    .llm(llm)
                    .config(Config::default())
                    .build();
                Arc::new(Mutex::new(agent))
            })
            .clone();

        Ok((session_id, agent))
    }

    /// 在指定 Agent 上运行一次对话，并将流式事件桥接到 Tauri Channel。
    pub async fn run_agent(
        &self,
        agent: Arc<Mutex<Agent>>,
        message: String,
        channel: Channel<AgentStreamEvent>,
    ) -> Result<(), AppError> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentStreamEvent>(64);

        // 桥接：内部 tokio channel -> Tauri Channel
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if channel.send(event).is_err() {
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

        Ok(())
    }
}

fn build_llm_from_env() -> Result<AgentsLLM, AppError> {
    let api_key = std::env::var("API_KEY")
        .map_err(|_| AppError::EnvVarMissing { name: "API_KEY" })?;
    let base_url = std::env::var("BASE_URL")
        .map_err(|_| AppError::EnvVarMissing { name: "BASE_URL" })?;
    let model = std::env::var("MODEL")
        .map_err(|_| AppError::EnvVarMissing { name: "MODEL" })?;
    let provider = std::env::var("PROVIDER").unwrap_or_else(|_| "Custom".into());

    Ok(AgentsLLM::builder()
        .api_key(api_key)
        .base_url(base_url)
        .model(model)
        .provider(provider)
        .build())
}
