// src/dag/node_internal/worker.rs
// Worker Agent 封装 — 接收输入执行工作任务
//
// Worker Agent 现在支持完整的 ReAct 循环（带工具调用）：
// 1. 构建系统提示 + 输入数据
// 2. 调用 call_llm_with_tools() — LLM 可调用 registered tools
// 3. 收集最终响应
// 4. 返回 WorkerOutput

use tokio::time::Instant;

use crate::dag::runtime::{call_llm_with_tools, DAGContext};
use crate::dag::types::{DAGResult, WorkerOutput};
use crate::model::ChatMessage;
use crate::tools::ToolManager;

/// Worker Agent 配置
pub struct WorkerConfig {
    /// Agent 名称
    pub name: String,
    /// 任务描述（作为系统提示）
    pub instruction: String,
    /// 输入数据（由上游提供）
    pub input: serde_json::Value,
    /// 最大执行轮次
    pub max_turns: usize,
    /// 前次审核反馈（重试时注入，帮助 Worker 修订输出）
    pub previous_feedback: Option<String>,
}

/// Worker Agent 封装
pub struct WorkerAgent;

impl WorkerAgent {
    /// 执行工作任务
    ///
    /// 1. 构建系统提示（instruction + input）
    /// 2. 调用 `call_llm_with_tools()` — 完整 ReAct 循环，支持工具调用
    /// 3. 收集响应
    /// 4. 返回 WorkerOutput
    pub async fn execute(
        ctx: &DAGContext,
        config: WorkerConfig,
    ) -> DAGResult<WorkerOutput> {
        let start = Instant::now();

        // 构建消息
        let feedback_section = match &config.previous_feedback {
            Some(feedback) => format!(
                "\n\n## 前次审核反馈（需要修订）\n{}\n\n请根据以上审核反馈修订你的输出。\n",
                feedback
            ),
            None => String::new(),
        };

        let system_prompt = format!(
            "你是 DAG Pipeline 中的 Worker Agent。\n\
你的任务是根据以下指令，使用输入数据完成工作。\n\
你可以使用提供的工具来辅助完成任务。\n\
\n\
## 任务指令\n\
{}\n\
\n\
## 输入数据\n\
{}\n\
{}",
            config.instruction,
            serde_json::to_string_pretty(&config.input).unwrap_or_default(),
            feedback_section,
        );

        let user_msg = if config.previous_feedback.is_some() {
            "请根据审核反馈修订你的输出，确保本次输出满足所有要求。"
        } else {
            "请根据指令完成上述任务，并输出最终结果。"
        };

        let mut messages = vec![
            ChatMessage::system(&system_prompt),
            ChatMessage::user(user_msg),
        ];

        // 使用带工具支持的 ReAct 循环调用 LLM
        // 工具通过 ctx.tool_manager 传入，Worker 可以执行工具调用
        let response = call_llm_with_tools(
            &ctx.model,
            &mut messages,
            &*ctx.tool_manager, // Arc<ToolManager> → &ToolManager
            config.max_turns,
        ).await?;

        let duration = start.elapsed().as_secs_f64();

        // 尝试解析 JSON
        let structured = serde_json::from_str::<serde_json::Value>(&response).ok();

        Ok(WorkerOutput {
            content: response,
            structured,
            execution_log: vec![],
            duration_secs: duration,
        })
    }
}
