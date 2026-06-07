// src/dag/runtime.rs
// DAGContext + NodeRuntime — DAG 执行上下文与节点执行器

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::Mutex;

use crate::dag::node::NodeDef;
use crate::dag::node_internal::supervisor::NodeSupervisor;
use crate::dag::types::{
    DAGResult, InputMode, MergeStrategy, WorkerOutput,
};
use crate::model::{ChatMessage, ModelAdapter, ModelEvent};
use crate::tools::ToolManager;

/// DAG 执行上下文 — 持有 Worker/Reviewer 所需的共享资源
pub struct DAGContext {
    /// 模型适配器（用于 LLM 调用）
    pub model: Arc<Mutex<Box<dyn ModelAdapter>>>,
    /// 工具管理器（Worker Agent 可使用）
    pub tool_manager: ToolManager,
}

impl DAGContext {
    /// 创建新的 DAG 上下文
    pub fn new(model: Box<dyn ModelAdapter>, tool_manager: ToolManager) -> Self {
        Self {
            model: Arc::new(Mutex::new(model)),
            tool_manager,
        }
    }

    /// 克隆上下文的轻量引用
    pub fn clone_light(&self) -> Self {
        Self {
            model: self.model.clone(),
            tool_manager: ToolManager::new(), // Worker 不共享工具（隔离执行）
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

/// 节点执行器 — 将 DAGEngine 与 NodeSupervisor 连接
pub struct NodeRuntime;

impl NodeRuntime {
    /// 执行一个节点，返回最终输出
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
            crate::dag::types::NodeResult::Success { output, .. } => {
                Ok(serde_json::json!({ "content": output }))
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
