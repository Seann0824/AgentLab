// src/dag/runtime.rs
// DAGContext + NodeRuntime — DAG 执行上下文与节点执行器
//
// 核心职责：
// 1. DAGContext — 共享执行上下文（Model + ToolManager）
// 2. call_llm — 简化版 LLM 调用（供 Reviewer 使用）
// 3. call_llm_with_tools — 完整 ReAct 循环 + 工具调用（供 Worker 使用）
// 4. NodeRuntime — 节点执行器

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::Mutex;

use crate::dag::node::NodeDef;
use crate::dag::node_internal::supervisor::NodeSupervisor;
use crate::dag::types::{
    DAGError, DAGResult, InputMode, MergeStrategy, WorkerOutput,
};
use crate::model::{ChatMessage, ModelAdapter, ModelEvent, ToolCall};
use crate::tools::ToolManager;

/// DAG 执行上下文 — 持有 Worker/Reviewer 所需的共享资源
#[derive(Clone)]
pub struct DAGContext {
    /// 模型适配器（用于 LLM 调用）
    pub model: Arc<Mutex<Box<dyn ModelAdapter>>>,
    /// 工具管理器（Worker Agent 可使用，Arc 实现共享）
    pub tool_manager: Arc<ToolManager>,
}

impl DAGContext {
    /// 创建新的 DAG 上下文
    pub fn new(model: Box<dyn ModelAdapter>, tool_manager: ToolManager) -> Self {
        Self {
            model: Arc::new(Mutex::new(model)),
            tool_manager: Arc::new(tool_manager),
        }
    }

    /// 克隆上下文的轻量引用（共享 model 和 tool_manager）
    pub fn clone_light(&self) -> Self {
        Self {
            model: self.model.clone(),
            tool_manager: self.tool_manager.clone(), // 共享工具管理器
        }
    }
}

/// 通过 LLM 发送消息并收集完整响应文本
pub async fn call_llm(
    model: &Arc<Mutex<Box<dyn ModelAdapter>>>,
    messages: Vec<ChatMessage>,
    tools: serde_json::Value,
) -> DAGResult<(String, serde_json::Value)> {
    let model = model.lock().await;
    let mut stream = model.stream_chat(&messages, tools);

    let mut response_text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ModelEvent::Text(content) => {
                response_text.push_str(&content);
            }
            ModelEvent::Thinking(_) => {
                // Worker/Reviewer 不输出 thinking
            }
            ModelEvent::Done(final_msg) => {
                response_text = final_msg;
            }
            ModelEvent::Error(err) => {
                return Err(crate::dag::types::DAGError::Internal(
                    format!("LLM 调用错误: {}", err)
                ));
            }
            ModelEvent::ToolCallBlock { .. } => {
                // 简版实现：不支持工具调用
                // Phase 2 增强版可添加工具循环
            }
        }
    }

    Ok((response_text, serde_json::json!({})))
}

// =====================================================================
// 带工具调用的 LLM ReAct 循环
// =====================================================================

/// 带工具支持的 LLM 调用 — 完整的 ReAct 循环
///
/// 工作流程:
/// 1. 调用 LLM（携带消息历史和工具 schema）
/// 2. 如果 LLM 返回 Text → 直接返回
/// 3. 如果 LLM 返回 ToolCallBlock → 执行工具 → 将结果追加到消息历史 → 回到步骤 1
/// 4. 达到 max_turns 后返回错误
///
/// # 参数
/// * `model` — 共享的模型适配器
/// * `messages` — 初始消息列表（会被追加 tool results）
/// * `tool_manager` — 工具管理器
/// * `max_turns` — 最大 ReAct 轮次（防止无限循环）
///
/// # 返回值
/// * `Ok(String)` — 最终的文本响应
pub async fn call_llm_with_tools(
    model: &Arc<Mutex<Box<dyn ModelAdapter>>>,
    messages: &mut Vec<ChatMessage>,
    tool_manager: &ToolManager,
    max_turns: usize,
) -> DAGResult<String> {
    for turn in 0..max_turns {
        let tools_schema = tool_manager.get_tools_scehma();

        let (response_text, tool_calls) = {
            let model_guard = model.lock().await;
            let mut stream = model_guard.stream_chat(messages, tools_schema);

            let mut text = String::new();
            let mut calls = Vec::new();

            while let Some(event) = stream.next().await {
                match event {
                    ModelEvent::Text(content) => {
                        text.push_str(&content);
                    }
                    ModelEvent::Thinking(_) => {
                        // 不输出 thinking
                    }
                    ModelEvent::ToolCallBlock { id, name, arguments } => {
                        calls.push(ToolCall { id, name, arguments });
                    }
                    ModelEvent::Done(final_msg) => {
                        text = final_msg;
                    }
                    ModelEvent::Error(err) => {
                        return Err(DAGError::Internal(format!("LLM 调用错误: {}", err)));
                    }
                }
            }
            (text, calls)
        };

        if tool_calls.is_empty() {
            // 没有工具调用 → 最终文本响应
            return Ok(response_text);
        }

        // 有工具调用 → 将 assistant 消息加入历史
        messages.push(ChatMessage::assistant_tool_calls(response_text, tool_calls.clone()));

        // 执行每个工具并将结果加入历史
        for tc in &tool_calls {
            let result = tool_manager.run(tc.clone()).await;
            messages.push(result);
        }
    }

    Err(DAGError::Internal(format!(
        "达到最大工具调用轮次 ({})，可能陷入循环", max_turns
    )))
}

/// 节点执行器 — 将 DAGEngine 与 NodeSupervisor 连接
pub struct NodeRuntime;

impl NodeRuntime {
    /// 执行一个节点，返回最终输出
    ///
    /// 返回值结构：
    /// ```json
    /// {
    ///   "content": "节点输出的文本内容",
    ///   "worker_output": "Worker 原始输出",
    ///   "review": { "passed": true, "score": 0.9, "feedback": "xxx", ... }
    /// }
    /// ```
    pub async fn execute_node(
        &self,
        ctx: &DAGContext,
        node_def: &NodeDef,
        input: serde_json::Value,
        max_retries: u32,
    ) -> DAGResult<serde_json::Value> {
        // 调用 NodeSupervisor 执行 Worker → Reviewer 流程
        let result = NodeSupervisor::execute_with_retry(ctx, node_def, input, max_retries).await?;

        match result {
            crate::dag::types::NodeResult::Success { output, review } => {
                // ReviewResult 结构使用 details 字段名，而非 check_results
                Ok(serde_json::json!({
                    "content": output,
                    "worker_output": output,
                    "review": {
                        "passed": review.passed,
                        "score": review.score,
                        "feedback": review.feedback,
                        "details": review.check_results,
                    },
                }))
            }
            crate::dag::types::NodeResult::FailedAfterRetries { last_review, retries, .. } => {
                Err(crate::dag::types::DAGError::Internal(
                    format!("节点执行失败（重试 {} 次后）: {}", retries, last_review.feedback)
                ))
            }
            crate::dag::types::NodeResult::NeedsRevision { .. } => {
                // 不应到达这里（execute_with_retry 会处理）
                Err(crate::dag::types::DAGError::Internal(
                    "意外的 NeedsRevision 状态".to_string()
                ))
            }
        }
    }
}
