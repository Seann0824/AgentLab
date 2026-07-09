use std::marker::PhantomData;

use openai_api_rs::v1::chat_completion::ToolChoiceType;
use serde::de::DeserializeOwned;

use crate::base::agent::{
    assistant_message_with_tools, system_message, tool_message, user_message, AgentBase,
};
use crate::base::llm::AgentsLLM;
use crate::tools::ToolManager;

const MAX_RETRIES: usize = 3;

/// 一种特殊的 Agent：内部绑定一个 `ToolManager`（通常只注册一个工具），
/// 并强制模型必须调用其中一个工具。
///
/// 返回值通过泛型 `T` 定义，由**工具执行后的返回值**反序列化得到。
/// 如果工具执行失败或反序列化失败，会自动把结果/错误喂回 LLM，最多重试 3 次。
pub struct ToolAgent<T> {
    base: AgentBase,
    tool_manager: ToolManager,
    _phantom: PhantomData<T>,
}

impl<T: DeserializeOwned> ToolAgent<T> {
    pub fn new(
        name: impl Into<String>,
        llm: AgentsLLM,
        system_prompt: impl Into<String>,
        tool_manager: ToolManager,
    ) -> Self {
        let base = AgentBase::new(name, llm, Some(system_prompt.into()), None);

        Self {
            base,
            tool_manager,
            _phantom: PhantomData,
        }
    }

    /// 执行一次单轮工具调用，并把**工具执行结果**反序列化为 `T`。
    ///
    /// 工具失败或结果无法解析时，会把错误信息加入对话历史并重新调用 LLM，最多重试 3 次。
    pub async fn run(&mut self, input: &str) -> Result<T, String> {
        let mut messages = vec![
            system_message(self.base.system_prompt.clone().unwrap_or_default()),
            user_message(input),
        ];

        let tools = self.tool_manager.get_tools_scehma();

        for attempt in 0..MAX_RETRIES {
            // 部分推理/Thinking 模型不支持 tool_choice: "required"，降级为 Auto。
            // 系统提示已强制要求调用工具，若模型未调用会在下方报错并重试。
            let resp = self
                .base
                .llm
                .chat_completion(messages.clone(), tools.clone(), ToolChoiceType::Auto)
                .await
                .map_err(|e| format!("[ToolAgent] LLM call failed: {}", e))?;

            let choice = resp
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| "[ToolAgent] LLM returned no choice".to_string())?;

            let tool_calls = choice
                .message
                .tool_calls
                .ok_or_else(|| "[ToolAgent] LLM did not call the required tool".to_string())?;

            let first_call = tool_calls
                .into_iter()
                .next()
                .ok_or_else(|| "[ToolAgent] empty tool calls".to_string())?;

            // 把 assistant 的工具调用请求加入历史。
            messages.push(assistant_message_with_tools("", vec![first_call.clone()]));

            // 真正执行工具。
            let (_, tool_call_id, tool_result) = self.tool_manager.run(first_call).await;
            let response_text = match &tool_result {
                Ok(content) => content.clone(),
                Err(error_msg) => error_msg.clone(),
            };

            // 把工具执行结果加入历史。
            messages.push(tool_message(tool_call_id, response_text.clone()));

            match tool_result {
                Ok(content) => match serde_json::from_str::<T>(&content) {
                    Ok(value) => return Ok(value),
                    Err(e) => {
                        if attempt + 1 == MAX_RETRIES {
                            return Err(format!(
                                "[ToolAgent] failed to deserialize tool output after {} attempts: {}",
                                MAX_RETRIES, e
                            ));
                        }
                        messages.push(user_message(format!(
                            "工具返回的结果格式不正确：{}。请重新调用工具并输出合法的 JSON。",
                            e
                        )));
                    }
                },
                Err(e) => {
                    if attempt + 1 == MAX_RETRIES {
                        return Err(format!(
                            "[ToolAgent] tool execution failed after {} attempts: {}",
                            MAX_RETRIES, e
                        ));
                    }
                    messages.push(user_message(format!(
                        "工具执行失败：{}。请修正后重新调用工具。",
                        e
                    )));
                }
            }
        }

        Err("[ToolAgent] exceeded max retries".to_string())
    }
}
