use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex as TokioMutex;

use crate::swarm::orchestrator::SwarmOrchestrator;
use crate::swarm::pool::AgentPoolManager;
use crate::swarm::registry::AgentType;
use crate::swarm::rpc::JsonRpcRequest;
use crate::swarm::task::{SwarmTask, TaskResult, TaskStatus};

pub(super) async fn execute_step(
    _pool_manager: Arc<TokioMutex<AgentPoolManager>>,
    orchestrator: Option<Arc<TokioMutex<SwarmOrchestrator>>>,
    task: &str,
    step_name: String,
    timeout_seconds: u64,
    retry_count: u32,
) -> Result<String> {
    let Some(orchestrator) = orchestrator else {
        return Err(anyhow::anyhow!(
            "Workflow 步骤 '{}' 需要 Orchestrator 才能真实执行",
            step_name
        ));
    };

    let mut last_error = None;
    for attempt in 0..=retry_count {
        eprintln!(
            "📋 [Workflow] 步骤 '{}' 通过 Orchestrator 派发 (attempt {}/{})",
            step_name,
            attempt + 1,
            retry_count + 1
        );

        let result = dispatch_once(orchestrator.clone(), task, &step_name, timeout_seconds).await;

        match result {
            Ok(output) => return Ok(output),
            Err(err) => {
                last_error = Some(err);
                if attempt < retry_count {
                    eprintln!("📋 [Workflow] 步骤 '{}' 将重试", step_name);
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Workflow 步骤 '{}' 未执行", step_name)))
}

async fn dispatch_once(
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    task: &str,
    step_name: &str,
    timeout_seconds: u64,
) -> Result<String> {
    let agent_id = wait_for_general_agent(orchestrator.clone())
        .await
        .ok_or_else(|| {
            anyhow::anyhow!(
                "没有可用的 General Agent 执行 Workflow 步骤 '{}'",
                step_name
            )
        })?;

    let swarm_task = SwarmTask::new(
        "workflow_step",
        serde_json::json!({
            "task_description": task,
            "task_params": {
                "workflow_step": step_name,
            }
        }),
    )
    .with_target("general")
    .with_timeout(timeout_seconds.max(1))
    .with_agent_id(agent_id.clone());

    let request = JsonRpcRequest::new("dispatch_task", Some(swarm_task.to_rpc_params()));
    let response = SwarmOrchestrator::send_request_and_wait_shared(
        orchestrator,
        &agent_id,
        &request,
        timeout_seconds.max(1),
    )
    .await
    .map_err(|e| anyhow::anyhow!(e))?;

    if let Some(error) = response.error {
        return Err(anyhow::anyhow!("Agent 响应错误: {}", error));
    }

    let task_result = response
        .result
        .and_then(|value| value.get("task_result").cloned())
        .and_then(|value| serde_json::from_value::<TaskResult>(value).ok())
        .ok_or_else(|| anyhow::anyhow!("Agent 响应缺少 task_result"))?;

    if task_result.status != TaskStatus::Completed {
        return Err(anyhow::anyhow!(
            "Workflow 步骤 '{}' 执行失败: {}",
            step_name,
            task_result
                .error
                .unwrap_or_else(|| "unknown error".to_string())
        ));
    }

    Ok(task_result
        .data
        .map(|data| data.to_string())
        .unwrap_or_else(|| "null".to_string()))
}

async fn wait_for_general_agent(
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
) -> Option<String> {
    for _ in 0..20 {
        let agent_id = {
            let orch = orchestrator.lock().await;
            orch.find_agent_by_type(&AgentType::General).await
        };
        if agent_id.is_some() {
            return agent_id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    None
}
