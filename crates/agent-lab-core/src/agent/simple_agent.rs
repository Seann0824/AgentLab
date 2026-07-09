use crate::{
    base::{
        agent::{
            assistant_message, assistant_message_with_tools, tool_message, user_message, Agent,
            AgentBase, AgentStreamEvent,
        },
        config::Config,
        llm::AgentsLLM,
    },
    services::chat_dto::ChatMessage,
    tools::{types::Tool, ToolManager},
};
use futures_util::stream::StreamExt;
use openai_api_rs::v1::chat_completion::{
    FinishReason, chat_completion_stream::ChatCompletionStreamResponse,
};

pub struct SimpleAgent {
    enable_tool_calling: bool,
    tool_manager: ToolManager,
    base: AgentBase,
}

impl SimpleAgent {
    pub fn new(
        name: impl Into<String>,
        llm: AgentsLLM,
        system_prompt: impl Into<Option<String>>,
        config: impl Into<Option<Config>>,
        tool_manager: impl Into<Option<ToolManager>>,
        enable_tool_calling: bool,
    ) -> Self {
        let config = config.into().unwrap_or_default();
        let system_prompt = system_prompt.into().unwrap_or("".into());
        let tool_manager = tool_manager.into().unwrap_or(ToolManager::new());
        let agent_base =
            AgentBase::new(name.into(), llm, Some(system_prompt.clone()), Some(config));

        Self {
            enable_tool_calling,
            tool_manager,
            base: agent_base,
        }
    }

    pub fn builder() -> AgentBuilder {
        AgentBuilder::new()
    }

    // 开放给外部注册工具
    pub fn add_tool(&mut self, tool: Box<dyn Tool + Send + Sync>) {
        self.tool_manager.register_tool(tool);
    }

    pub fn remove_tool(&mut self, tool_name: &String) {
        self.tool_manager.remove_tool(tool_name);
    }
}

pub struct AgentBuilder {
    name: Option<String>,
    llm: Option<AgentsLLM>,
    system_prompt: Option<String>,
    config: Option<Config>,
    tool_manager: ToolManager,
    enable_tool_calling: bool,
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            name: None,
            llm: None,
            system_prompt: None,
            config: None,
            tool_manager: ToolManager::new(),
            enable_tool_calling: false,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn llm(mut self, llm: AgentsLLM) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn system_prompt(mut self, system_prompt: impl Into<Option<String>>) -> Self {
        self.system_prompt = system_prompt.into();
        self
    }

    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    pub fn tool(mut self, tool: Box<dyn Tool + Send + Sync>) -> Self {
        self.tool_manager.register_tool(tool);
        self
    }

    pub fn enable_tool_calling(mut self, enable: bool) -> Self {
        self.enable_tool_calling = enable;
        self
    }

    pub fn build(self) -> SimpleAgent {
        let name = self
            .name
            .expect("AgentBuilder: name is required, use .name(...) to set it");
        let llm = self
            .llm
            .expect("AgentBuilder: llm is required. Use .llm(AgentsLLM::builder()...build())");
        let config = self
            .config
            .expect("AgentBuilder: config is required. Use .config(Config::builder()...build()) or .config(Config::default())");
        let system_prompt = self.system_prompt.unwrap_or_default();

        SimpleAgent::new(
            name,
            llm,
            Some(system_prompt),
            Some(config),
            Some(self.tool_manager),
            self.enable_tool_calling,
        )
    }
}

#[async_trait::async_trait]
impl Agent for SimpleAgent {
    fn base(&self) -> &AgentBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AgentBase {
        &mut self.base
    }

    async fn run(&mut self, input_text: &str) -> String {
        self.base.ensure_system_prompt();

        let user_id = uuid::Uuid::new_v4().to_string();
        let user_msg = user_message(input_text);
        self.add_message(user_msg.clone());
        self.base
            .emit(AgentStreamEvent::UserMessage {
                message: ChatMessage::from_chat_completion_message_now(&user_msg, user_id),
            })
            .await;

        let mut is_continue = true;
        let mut final_response = String::new();

        loop {
            if !is_continue {
                break;
            }
            let history_message = self.base.get_history();
            let mut agent_stream = self
                .base
                .llm
                .invoke(
                    history_message,
                    self.enable_tool_calling
                        .then(|| self.tool_manager.get_tools_scehma()),
                    None,
                )
                .await;
            let (mut reason_delta, mut content_delta) = (vec![], vec![]);
            let mut current_assistant_message_id: Option<String> = None;
            let mut tools_call = None;

            while let Some(chunck) = agent_stream.next().await {
                match chunck {
                    ChatCompletionStreamResponse::Content(delta) => {
                        if current_assistant_message_id.is_none() {
                            current_assistant_message_id = Some(uuid::Uuid::new_v4().to_string());
                        }
                        self.base
                            .emit(AgentStreamEvent::AssistantDelta {
                                message_id: current_assistant_message_id.clone().unwrap(),
                                delta: delta.clone(),
                            })
                            .await;
                        content_delta.push(delta);
                    }
                    ChatCompletionStreamResponse::Reasoning(delta) => {
                        self.base
                            .emit(AgentStreamEvent::ReasonDelta {
                                delta: delta.clone(),
                            })
                            .await;
                        reason_delta.push(delta);
                    }
                    ChatCompletionStreamResponse::ToolCall(tc) => {
                        tools_call = Some(tc);
                    }
                    ChatCompletionStreamResponse::Done(finish_reason) => {
                        let Some(finish_reason) = finish_reason else {
                            panic!("end failed");
                        };

                        let assistant_id = current_assistant_message_id
                            .clone()
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                        match finish_reason {
                            FinishReason::tool_calls | FinishReason::function_call => {
                                let tools_call = tools_call.clone().unwrap_or_default().clone();
                                // 如果模型声明要调用工具但没有给出具体 tool_call，降级为普通内容回复。
                                if tools_call.is_empty() {
                                    let content = content_delta.join("");
                                    if !content.is_empty() {
                                        let msg = assistant_message(content.clone());
                                        self.add_message(msg);
                                        self.base
                                            .emit(AgentStreamEvent::AssistantDone {
                                                message: ChatMessage::from_chat_completion_message_now(
                                                    &assistant_message(content.clone()),
                                                    assistant_id,
                                                ),
                                            })
                                            .await;
                                        final_response = content;
                                    }
                                    is_continue = false;
                                    break;
                                }

                                // 部分模型/提供商不接受 content 和 tool_calls 同时为空的 assistant 消息。
                                // 如果 reasoning 内容为空，则不加入历史，避免后续请求被 API 拒绝。
                                let reasoning = reason_delta.join("");
                                if !reasoning.is_empty() {
                                    let reasoning_msg = assistant_message(reasoning.clone());
                                    self.add_message(reasoning_msg);
                                    self.base
                                        .emit(AgentStreamEvent::AssistantDone {
                                            message: ChatMessage::from_chat_completion_message_now(
                                                &assistant_message(reasoning),
                                                assistant_id.clone(),
                                            ),
                                        })
                                        .await;
                                }

                                let assistant_tool_msg = assistant_message_with_tools(
                                    content_delta.join(""),
                                    tools_call.clone(),
                                );
                                self.add_message(assistant_tool_msg.clone());
                                self.base
                                    .emit(AgentStreamEvent::AssistantDone {
                                        message: ChatMessage::from_chat_completion_message_now(
                                            &assistant_tool_msg,
                                            assistant_id,
                                        ),
                                    })
                                    .await;

                                // 对每个 tool_call 发送开始事件，然后并发执行。
                                for tc in &tools_call {
                                    self.base
                                        .emit(AgentStreamEvent::ToolCallStart {
                                            tool_call_id: tc.id.clone(),
                                            tool_name: tc
                                                .function
                                                .name
                                                .clone()
                                                .unwrap_or_else(|| "内部工具".to_string()),
                                            arguments: tc.function.arguments.clone().unwrap_or_default(),
                                        })
                                        .await;
                                }

                                let tasks = tools_call
                                    .iter()
                                    .map(|tool_call| self.tool_manager.run(tool_call.clone()))
                                    .collect::<Vec<_>>();

                                let tools_call_result = futures_util::future::join_all(tasks).await;

                                for (tool_name, tool_call_id, tool_call_result) in
                                    tools_call_result.into_iter()
                                {
                                    let mut is_error = false;
                                    let tool_call_result_content = match tool_call_result {
                                        Ok(content) => content,
                                        Err(error_msg) => {
                                            is_error = true;
                                            error_msg
                                        }
                                    };

                                    self.base
                                        .emit(AgentStreamEvent::ToolCallEnd {
                                            tool_call_id: tool_call_id.clone(),
                                            tool_name: tool_name.clone(),
                                            result: tool_call_result_content.clone(),
                                            is_error,
                                        })
                                        .await;

                                    let tool_msg =
                                        tool_message(tool_call_id, tool_call_result_content);
                                    self.add_message(tool_msg);
                                }
                            }
                            _ => {
                                let content = content_delta.join("");
                                let reasoning = reason_delta.join("");
                                if !reasoning.is_empty() {
                                    self.base
                                        .emit(AgentStreamEvent::ReasonDone {
                                            reason: reasoning.clone(),
                                        })
                                        .await;
                                }

                                if !reasoning.is_empty() {
                                    let reasoning_msg = assistant_message(reasoning.clone());
                                    self.add_message(reasoning_msg);
                                    self.base
                                        .emit(AgentStreamEvent::AssistantDone {
                                            message: ChatMessage::from_chat_completion_message_now(
                                                &assistant_message(reasoning),
                                                assistant_id.clone(),
                                            ),
                                        })
                                        .await;
                                }

                                let final_msg = assistant_message(content.clone());
                                self.add_message(final_msg);
                                self.base
                                    .emit(AgentStreamEvent::AssistantDone {
                                        message: ChatMessage::from_chat_completion_message_now(
                                            &assistant_message(content.clone()),
                                            assistant_id,
                                        ),
                                    })
                                    .await;

                                final_response = content;
                                is_continue = false;
                                break;
                            }
                        }
                    }
                }
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        }

        final_response
    }
}
