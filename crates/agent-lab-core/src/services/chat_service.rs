use std::collections::HashMap;
use std::sync::Arc;

use chrono::Local;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::agent::simple_agent::{AgentBuilder, SimpleAgent};
use crate::base::agent::{Agent as AgentTrait, AgentStreamEvent};
use crate::base::config::Config;
use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::services::chat_dto::{ChatMessage, SessionSummary};
use crate::services::{MessageService, SessionService};
use crate::tools::time_tool::TimeTool;
use crate::tools::web_search::WebSearch;
use openai_api_rs::v1::chat_completion::ChatCompletionMessage;

pub type SessionId = String;

/// 聊天业务服务：管理会话生命周期与 Agent 运行。
///
/// 所有会话与消息状态均持久化到 PostgreSQL；本服务本身不持有内存历史，
/// 每次发送消息时从 DB 加载历史、新建 Agent、运行、并通过事件回写新消息。
pub struct ChatService {
    sessions: SessionService,
    messages: MessageService,
    llm: AgentsLLM,
    user_id: String,
    /// 按 session 串行化发送，防止同一会话并发产生交错消息。
    send_locks: Mutex<HashMap<SessionId, Arc<Mutex<()>>>>,
}

impl ChatService {
    pub fn new(
        llm: AgentsLLM,
        sessions: SessionService,
        messages: MessageService,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            sessions,
            messages,
            llm,
            user_id: user_id.into(),
            send_locks: Mutex::new(HashMap::new()),
        }
    }

    fn build_agent(llm: AgentsLLM) -> SimpleAgent {
        let time_tool = Box::new(TimeTool::new());
        let search_tool = Box::new(WebSearch::serpapi(
            std::env::var("SERPAPI_API_KEY").expect("SERPAPI_API_KEY missing"),
        ));
        AgentBuilder::new()
            .name("chat agent")
            .llm(llm)
            .config(Config::default())
            .tool(time_tool)
            .tool(search_tool)
            .enable_tool_calling(true)
            .build()
    }

    /// 创建新会话并返回 session_id。
    pub async fn create_session(&self) -> Result<String, AgentLabError> {
        let session_id = Uuid::new_v4().to_string();
        self.sessions.create(&self.user_id, &session_id).await?;
        Ok(session_id)
    }

    /// 获取已有会话，或在 session_id 为空/不存在时创建新会话。
    async fn get_or_create_session(
        &self,
        session_id: Option<String>,
    ) -> Result<String, AgentLabError> {
        if let Some(id) = session_id {
            if self.sessions.get(&id).await?.is_some() {
                return Ok(id);
            }
        }
        self.create_session().await
    }

    /// 列出所有会话摘要。
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, AgentLabError> {
        let mut summaries = self.sessions.list(&self.user_id).await?;

        // 对没有标题的会话，用第一条用户消息作为标题。
        for summary in &mut summaries {
            if summary.title == "新会话" {
                if let Ok(messages) = self.messages.history(&summary.id).await {
                    if let Some(first_user) = messages.iter().find(|m| m.role == "user") {
                        let content = &first_user.content;
                        summary.title = if content.chars().count() > 20 {
                            format!("{}...", content.chars().take(20).collect::<String>())
                        } else {
                            content.clone()
                        };
                    }
                }
            }
        }

        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }

    /// 获取指定会话的历史消息。
    pub async fn get_session_history(
        &self,
        session_id: &str,
    ) -> Result<Vec<ChatMessage>, AgentLabError> {
        Ok(self.messages.history(session_id).await?)
    }

    /// 删除会话。
    pub async fn delete_session(&self, session_id: &str) -> Result<bool, AgentLabError> {
        Ok(self.sessions.delete(session_id).await?)
    }

    /// 重命名会话。
    pub async fn rename_session(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<bool, AgentLabError> {
        Ok(self.sessions.rename(session_id, title).await?)
    }

    /// 在指定会话中运行 Agent，并将流式事件桥接到内部 channel。
    ///
    /// 流程：
    /// 1. 从 DB 加载历史；
    /// 2. 新建 Agent 并注入历史；
    /// 3. Agent 运行期间产生的事件会同时转发给调用方并持久化到 DB。
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        message: String,
        channel: mpsc::Sender<AgentStreamEvent>,
    ) -> Result<String, AgentLabError> {
        let session_id = self.get_or_create_session(session_id).await?;
        let history = self.messages.history(&session_id).await?;

        // 获取该 session 的串行化锁。
        let lock = {
            let mut locks = self.send_locks.lock().await;
            locks
                .entry(session_id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let sessions = self.sessions.clone();
        let messages = self.messages.clone();
        let llm = self.llm.clone();
        let session_id_for_task = session_id.clone();

        tokio::spawn(async move {
            // 锁在发送/事件处理期间一直持有，任务结束自动释放。
            let _guard = lock.lock().await;

            // 1. 新建 Agent 并还原历史。
            let mut agent = Self::build_agent(llm);
            let history_cc: Vec<ChatCompletionMessage> = history
                .iter()
                .map(ChatMessage::to_chat_completion_message)
                .collect();
            agent.base_mut().set_history(history_cc);
            agent.base_mut().ensure_system_prompt();

            // 2. 建立内部事件 channel。
            let (tx, mut rx) = mpsc::channel::<AgentStreamEvent>(64);
            agent.base_mut().set_event_sender(Some(tx));

            // 3. 运行 Agent。
            tokio::spawn(async move {
                let _ = agent.run(&message).await;
            });

            // 4. 监听事件：转发 + 持久化。
            while let Some(event) = rx.recv().await {
                let event_for_persist = event.clone();

                // 转发给调用方。
                if channel.send(event).await.is_err() {
                    break;
                }

                // 持久化新消息。
                match event_for_persist {
                    AgentStreamEvent::UserMessage { message: msg } => {
                        let _ = messages.add(&session_id_for_task, &msg).await;
                        let _ = sessions.touch(&session_id_for_task).await;
                    }
                    AgentStreamEvent::AssistantDone { message: msg } => {
                        let _ = messages.add(&session_id_for_task, &msg).await;
                        let _ = sessions.touch(&session_id_for_task).await;
                    }
                    AgentStreamEvent::ToolCallEnd {
                        tool_call_id,
                        result,
                        is_error,
                        ..
                    } => {
                        let tool_msg = ChatMessage {
                            id: Uuid::new_v4().to_string(),
                            role: "tool".to_string(),
                            content: result,
                            timestamp: Local::now().timestamp(),
                            tool_call_id: Some(tool_call_id),
                            tool_calls: None,
                            metadata: Some(serde_json::json!({ "is_error": is_error })),
                        };
                        let _ = messages.add(&session_id_for_task, &tool_msg).await;
                        let _ = sessions.touch(&session_id_for_task).await;
                    }
                    _ => {}
                }
            }
        });

        Ok(session_id)
    }
}
