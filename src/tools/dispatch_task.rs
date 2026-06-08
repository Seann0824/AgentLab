// src/tools/dispatch_task.rs
// 📨 dispatch_task 工具 — 向蜂群中的 Agent 派发任务
//
// 这是连接 Orchestrator 与子 Agent 的核心工具。
// 通过 UDS 向已连接的 Agent 发送 JSON-RPC 请求，并等待执行结果。
//
// 设计文档: docs/analyses/swarm-architecture-gaps-analysis/04-code-paths-implementation.md

use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::swarm::orchestrator::SwarmOrchestrator;
use crate::swarm::registry::{AgentType, SwarmRegistry};
use crate::swarm::rpc::JsonRpcRequest;
use crate::swarm::task::{SwarmTask, TaskResult};
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// dispatch_task 工具
///
/// 向指定 Agent 类型派发任务，等待执行结果并返回。
///
/// # 参数
/// - `agent_type`: 目标 Agent 类型（orchestrator/memory/general/verifier/coder/researcher）
/// - `task_description`: 任务描述文本
/// - `task_params`: 可选的任务参数 JSON
/// - `timeout_seconds`: 超时秒数（默认 60）
pub struct DispatchTask {
    /// SwarmOrchestrator 的共享引用（用于发送请求）
    pub orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    /// SwarmRegistry 的共享引用（用于查找 Agent）
    pub registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
}

impl Tool for DispatchTask {
    fn name(&self) -> &str {
        "dispatch_task"
    }

    fn description(&self) -> &str {
        "向蜂群中的 Agent 派发任务。通过 UDS 向已连接的 Agent（memory/general/verifier/coder/researcher）发送任务并等待结果。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "dispatch_task",
                "description": "向指定 Agent 类型派发任务，等待执行结果。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "agent_type": {
                            "type": "string",
                            "description": "目标 Agent 类型: orchestrator/memory/general/verifier/coder/researcher",
                            "enum": ["memory", "general", "verifier", "coder", "researcher"]
                        },
                        "task_description": {
                            "type": "string",
                            "description": "任务描述，告诉 Agent 需要做什么"
                        },
                        "task_params": {
                            "type": "object",
                            "description": "可选的任务参数（JSON 格式）",
                            "default": {}
                        },
                        "timeout_seconds": {
                            "type": "integer",
                            "description": "等待 Agent 响应的超时秒数（默认 120）",
                            "default": 120
                        }
                    },
                    "required": ["agent_type", "task_description"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let agent_type_str = args["agent_type"].as_str().unwrap_or("general").to_string();
        let task_description = args["task_description"].as_str().unwrap_or("").to_string();
        let task_params = args
            .get("task_params")
            .cloned()
            .unwrap_or(serde_json::json!({}));
        let timeout_secs = args["timeout_seconds"].as_u64().unwrap_or(120);

        let (tx, rx) = mpsc::channel(1);
        let agent_type = parse_agent_type(&agent_type_str);
        dispatch_to_agent(
            agent_type,
            &agent_type_str,
            &task_description,
            task_params,
            timeout_secs,
            self.orchestrator.clone(),
            self.registry.clone(),
            tx,
        );
        Box::pin(ReceiverStream::new(rx))
    }
}

/// 提取为公共函数，供 DispatchTask 和 AgentTaskTool 共享使用
pub fn dispatch_to_agent(
    agent_type: AgentType,
    agent_type_str: &str,
    task_description: &str,
    task_params: serde_json::Value,
    timeout_secs: u64,
    orch: Arc<TokioMutex<SwarmOrchestrator>>,
    registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
    tx: mpsc::Sender<ToolEvent>,
) {
    let agent_type_str = agent_type_str.to_string();
    let task_description = task_description.to_string();
    tokio::spawn(async move {
        // 参数校验
        if task_description.trim().is_empty() {
            let _ = tx
                .send(ToolEvent::Err("task_description 不能为空".to_string()))
                .await;
            return;
        }

        if agent_type == AgentType::Orchestrator {
            let _ = tx
                .send(ToolEvent::Err(
                    "不能向 Orchestrator 自身派发任务".to_string(),
                ))
                .await;
            return;
        }

        let _ = tx
            .send(ToolEvent::Progress(format!(
                "📨 向 {} Agent 派发任务...",
                agent_type_str
            )))
            .await;

        // 查找目标 Agent ID
        let agent_id = find_agent_id(&registry, &agent_type, &orch).await;

        let agent_id = match agent_id {
            Some(id) => id,
            None => {
                let _ = tx
                    .send(ToolEvent::Err(format!(
                        "没有可用的 {} Agent。请先启动: agent-lab --agent-type {}",
                        agent_type_str, agent_type_str
                    )))
                    .await;
                return;
            }
        };

        let _ = tx
            .send(ToolEvent::Progress(format!(
                "📨 已找到 Agent '{}'，正在发送任务...",
                agent_id
            )))
            .await;

        // 构建结构化蜂群任务
        let swarm_task = SwarmTask::new(
            "dispatch_task",
            serde_json::json!({
                "task_description": task_description,
                "task_params": task_params,
            }),
        )
        .with_target(agent_type_str.clone())
        .with_timeout(timeout_secs)
        .with_agent_id(agent_id.clone());

        let request = JsonRpcRequest::new("dispatch_task", Some(swarm_task.to_rpc_params()));

        // 通过 Orchestrator 发送并等待响应
        match SwarmOrchestrator::send_request_and_wait_shared(
            orch.clone(),
            &agent_id,
            &request,
            timeout_secs,
        )
        .await
        {
            Ok(response) => {
                let task_result = response
                    .result
                    .as_ref()
                    .and_then(|value| {
                        value
                            .get("task_result")
                            .cloned()
                            .or_else(|| {
                                value
                                    .get("result")
                                    .and_then(|v| v.get("task_result"))
                                    .cloned()
                            })
                            .or_else(|| Some(value.clone()))
                    })
                    .and_then(|value| serde_json::from_value::<TaskResult>(value).ok());
                let result = serde_json::json!({
                    "agent_id": agent_id,
                    "agent_type": agent_type_str,
                    "task": swarm_task,
                    "response_id": response.id,
                    "response_result": response.result,
                    "response_error": response.error.map(|e| serde_json::json!({
                        "code": e.code,
                        "message": e.message,
                        "data": e.data,
                    })),
                    "task_result": task_result,
                });
                let _ = tx.send(ToolEvent::Done(result)).await;
            }
            Err(err_msg) => {
                let _ = tx
                    .send(ToolEvent::Err(format!("任务执行失败: {}", err_msg)))
                    .await;
            }
        }
    });
}

/// 解析 Agent 类型字符串
pub fn parse_agent_type(s: &str) -> AgentType {
    match s.to_lowercase().as_str() {
        "orchestrator" => AgentType::Orchestrator,
        "memory" => AgentType::Memory,
        "general" => AgentType::General,
        "verifier" => AgentType::Verifier,
        "coder" => AgentType::Coder,
        "researcher" => AgentType::Researcher,
        "reader" => AgentType::Reader,
        other => AgentType::Custom(other.to_string()),
    }
}

/// 从 Registry 或 Orchestrator 中查找可用的 Agent ID
async fn find_agent_id(
    _registry: &Option<Arc<TokioMutex<SwarmRegistry>>>,
    agent_type: &AgentType,
    orch: &Arc<TokioMutex<SwarmOrchestrator>>,
) -> Option<String> {
    let orch_lock = orch.lock().await;
    orch_lock.find_agent_by_type(agent_type).await
}
