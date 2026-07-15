use std::collections::HashMap;
use std::sync::Arc;

use chrono::Local;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::agent::simple_agent::{AgentBuilder, SimpleAgent};
use crate::base::agent::{Agent as AgentTrait, AgentStreamEvent};
use crate::base::config::Config;
use crate::base::llm::AgentsLLM;
use crate::base::provider_config::ModelSelection;
use crate::error::AgentLabError;
use crate::services::chat_dto::{ChatMessage, SessionSummary};
use crate::services::{MessageService, ProviderResolver, RagService, SessionService};
use crate::tools::memory::MemoryTool;
use crate::tools::rag::RagTool;
use crate::tools::shell::ShellTool;
use crate::tools::time::TimeTool;
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
    rag_service: Option<RagService>,
    memory_tool: Option<MemoryTool>,
    /// 按 session 串行化发送，防止同一会话并发产生交错消息。
    send_locks: Mutex<HashMap<SessionId, Arc<Mutex<()>>>>,
    /// Provider 解析器，用于根据前端传入的 model_selection 动态切换 LLM。
    resolver: Option<Arc<dyn ProviderResolver>>,
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
            rag_service: None,
            memory_tool: None,
            send_locks: Mutex::new(HashMap::new()),
            resolver: None,
        }
    }

    /// 注入 RAG 服务；注入后 Agent 才能使用 rag 工具。
    pub fn with_rag_service(mut self, rag_service: RagService) -> Self {
        self.rag_service = Some(rag_service);
        self
    }

    /// 注入记忆工具；注入后 Agent 才能使用 memory 工具管理长期记忆。
    pub fn with_memory_tool(mut self, memory_tool: MemoryTool) -> Self {
        self.memory_tool = Some(memory_tool);
        self
    }

    /// 注入 Provider 解析器；注入后 `send_message` 才能根据 `model_selection` 切换模型。
    pub fn with_resolver(mut self, resolver: impl ProviderResolver + 'static) -> Self {
        self.resolver = Some(Arc::new(resolver));
        self
    }

    fn build_agent(
        &self,
        llm: AgentsLLM,
        system_prompt: Option<String>,
        default_namespace: Option<String>,
        memory_enabled: bool,
    ) -> SimpleAgent {
        let time_tool = Box::new(TimeTool::new());
        let search_tool = Box::new(WebSearch::serpapi(
            std::env::var("SERPAPI_API_KEY").expect("SERPAPI_API_KEY missing"),
        ));
        let shell_tool = Box::new(ShellTool::new());

        let mut builder = AgentBuilder::new()
            .name("chat agent")
            .llm(llm)
            .config(Config::default())
            .system_prompt(system_prompt)
            .tool(time_tool)
            .tool(search_tool)
            .tool(shell_tool)
            .enable_tool_calling(true);

        if let Some(rag_service) = &self.rag_service {
            let rag_tool = Box::new(
                RagTool::with_service(rag_service.clone())
                    .with_default_namespace(default_namespace),
            );
            builder = builder.tool(rag_tool);
        }

        // 仅在开关开启且 memory_tool 已初始化成功时才注册记忆工具。
        if memory_enabled {
            if let Some(memory_tool) = &self.memory_tool {
                builder = builder.tool(Box::new(memory_tool.clone()));
            }
        }

        builder.build()
    }

    /// 构建包含知识库感知的 system prompt。
    fn build_knowledge_prompt(available_namespaces: &[String]) -> Option<String> {
        if available_namespaces.is_empty() {
            return None;
        }

        let namespaces_list = available_namespaces
            .iter()
            .map(|ns| format!("- {}", ns))
            .collect::<Vec<_>>()
            .join("\n");

        Some(format!(
            "你有权访问以下知识库 namespace：\n{}\n\
             当用户问题与某个知识库相关时，请使用 rag 工具的 search action，并在参数中指定要检索的 namespace。\
             如果用户消息中提到某个 namespace（如 @[namespace]），请优先在该 namespace 下检索。\
             如果问题可能涉及多个知识库，请选择最合适的 namespace 进行检索。\
             如果问题与知识库无关，直接回答即可。",
            namespaces_list
        ))
    }

    /// 构建记忆相关的 system prompt。
    fn build_memory_prompt() -> Option<String> {
        Some(
            "你拥有长期记忆能力。在对话过程中：\n\
             - 当用户提到值得长期保留的信息（如个人偏好、重要事实、计划、关键经历等）时，\
               你可以调用 memory 工具的 add action 将其保存到记忆中。\n\
             - 在回答用户问题前，如果认为相关历史记忆可能有助于回答，\
               可以调用 memory 工具的 search action 检索相关记忆。\n\
             - 不需要频繁保存每一条对话，只保存你认为对未来对话真正有价值的内容。"
                .to_string(),
        )
    }

    /// 组合知识库与记忆相关的 system prompt。
    fn build_system_prompt(
        &self,
        available_namespaces: &[String],
        memory_enabled: bool,
    ) -> Option<String> {
        let knowledge = Self::build_knowledge_prompt(available_namespaces);
        let memory = if memory_enabled && self.memory_tool.is_some() {
            Self::build_memory_prompt()
        } else {
            None
        };

        match (knowledge, memory) {
            (Some(k), Some(m)) => Some(format!("{}\n\n{}", k, m)),
            (Some(k), None) => Some(k),
            (None, Some(m)) => Some(m),
            (None, None) => None,
        }
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
    /// 2. 查询所有可用知识库 namespace 与记忆开关，构建 system_prompt；
    /// 3. 新建 Agent 并注入历史；
    /// 4. Agent 运行期间产生的事件会同时转发给调用方并持久化到 DB。
    ///
    /// 注意：用户消息中的 `@[namespace]` 仅作为普通 prompt 文本，
    /// 不在这里做特殊提取；AI 会根据 system_prompt 中的知识库列表自主调用 rag 工具。
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        message: String,
        channel: mpsc::Sender<AgentStreamEvent>,
        model_selection: Option<ModelSelection>,
        memory_enabled: bool,
    ) -> Result<String, AgentLabError> {
        let session_id = self.get_or_create_session(session_id).await?;
        let history = self.messages.history(&session_id).await?;

        // 查询可用 namespace 列表，构建知识库与记忆感知的 system prompt。
        let available_namespaces = if let Some(rag) = &self.rag_service {
            rag.list_namespaces().await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let system_prompt = self.build_system_prompt(&available_namespaces, memory_enabled);

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
        let llm = match model_selection {
            Some(selection) => self
                .resolver
                .as_ref()
                .ok_or_else(|| {
                    AgentLabError::ProviderConfig(
                        "未配置 ProviderResolver，无法使用 model_selection".into(),
                    )
                })?
                .resolve(&selection)?,
            None => self.llm.clone(),
        };
        let session_id_for_task = session_id.clone();

        // 在闭包外构建 Agent，避免在 async move 中捕获 self。
        let mut agent = self.build_agent(llm.clone(), system_prompt, None, memory_enabled);
        let history_cc: Vec<ChatCompletionMessage> = history
            .iter()
            .map(ChatMessage::to_chat_completion_message)
            .collect();
        agent.base_mut().set_history(history_cc);
        agent.base_mut().ensure_system_prompt();

        tokio::spawn(async move {
            // 锁在发送/事件处理期间一直持有，任务结束自动释放。
            let _guard = lock.lock().await;

            // 1. 建立内部事件 channel。
            let (tx, mut rx) = mpsc::channel::<AgentStreamEvent>(64);
            agent.base_mut().set_event_sender(Some(tx));

            // 2. 运行 Agent。
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
