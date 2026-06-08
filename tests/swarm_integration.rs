use std::sync::Arc;
use std::time::Duration;

use agent_lab::swarm::{
    SwarmOrchestrator,
    agents::{GeneralAgent, VerifierAgent},
    pool::AgentPoolManager,
    registry::AgentType,
    rpc::JsonRpcRequest,
    task::{SwarmTask, TaskResult, TaskStatus},
    workflow::{ExecutionMode, Workflow, WorkflowEngine, WorkflowStatus, WorkflowStep},
};
use tokio::sync::Mutex as TokioMutex;

fn test_socket(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "agent-lab-{}-{}-{}.sock",
        name,
        std::process::id(),
        now_nanos()
    ))
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

async fn wait_for_agent(
    orch: &Arc<TokioMutex<SwarmOrchestrator>>,
    agent_type: AgentType,
) -> String {
    for _ in 0..40 {
        let snapshot = {
            let orch = orch.lock().await;
            orch.get_registry_snapshot().await
        };
        if let Some(agent) = snapshot.query_by_type(&agent_type).first() {
            return agent.agent_id.clone();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {:?} agent", agent_type);
}

fn parse_task_result(response: agent_lab::swarm::JsonRpcResponse) -> TaskResult {
    response
        .result
        .and_then(|value| value.get("task_result").cloned())
        .and_then(|value| serde_json::from_value::<TaskResult>(value).ok())
        .expect("response should contain a task_result")
}

#[tokio::test]
async fn orchestrator_registers_agent_type_and_dispatches_general_task() {
    let socket = test_socket("general");
    let orchestrator = SwarmOrchestrator::bind(Some(socket.clone())).await.unwrap();
    let orch = Arc::new(TokioMutex::new(orchestrator));
    let accept_handle = SwarmOrchestrator::start_accept_loop(orch.clone());

    let mut general = GeneralAgent::new();
    general.connect(Some(socket.clone())).await.unwrap();
    let agent_handle = tokio::spawn(async move { general.run().await });

    let agent_id = wait_for_agent(&orch, AgentType::General).await;
    let task = SwarmTask::new(
        "integration_test",
        serde_json::json!({
            "task_description": "record this integration test task",
            "task_params": { "source": "swarm_integration" }
        }),
    )
    .with_target("general")
    .with_agent_id(agent_id.clone())
    .with_timeout(5);
    let request = JsonRpcRequest::new("dispatch_task", Some(task.to_rpc_params()));

    let response =
        SwarmOrchestrator::send_request_and_wait_shared(orch.clone(), &agent_id, &request, 5)
            .await
            .unwrap();
    let task_result = parse_task_result(response);

    assert_eq!(task_result.status, TaskStatus::Completed);
    let data = task_result.data.expect("task should return data");
    assert_eq!(data["success"].as_bool(), Some(true));
    assert_eq!(data["agent_id"].as_str(), Some(agent_id.as_str()));

    agent_handle.abort();
    accept_handle.abort();
    let _ = tokio::fs::remove_file(socket).await;
}

#[tokio::test]
async fn verifier_dispatch_runs_real_cargo_check() {
    let socket = test_socket("verifier");
    let orchestrator = SwarmOrchestrator::bind(Some(socket.clone())).await.unwrap();
    let orch = Arc::new(TokioMutex::new(orchestrator));
    let accept_handle = SwarmOrchestrator::start_accept_loop(orch.clone());

    let mut verifier = VerifierAgent::new(Some(std::path::PathBuf::from(".")));
    verifier.connect(Some(socket.clone())).await.unwrap();
    let agent_handle = tokio::spawn(async move { verifier.run().await });

    let agent_id = wait_for_agent(&orch, AgentType::Verifier).await;
    let task = SwarmTask::new(
        "integration_test",
        serde_json::json!({
            "task_description": "run cargo check",
            "task_params": {}
        }),
    )
    .with_target("verifier")
    .with_agent_id(agent_id.clone())
    .with_timeout(60);
    let request = JsonRpcRequest::new("dispatch_task", Some(task.to_rpc_params()));

    let response =
        SwarmOrchestrator::send_request_and_wait_shared(orch.clone(), &agent_id, &request, 60)
            .await
            .unwrap();
    let task_result = parse_task_result(response);

    assert_eq!(task_result.status, TaskStatus::Completed);
    let data = task_result.data.expect("cargo check should return data");
    assert_eq!(data["passed"].as_bool(), Some(true));

    agent_handle.abort();
    accept_handle.abort();
    let _ = tokio::fs::remove_file(socket).await;
}

#[tokio::test]
async fn workflow_engine_dispatches_real_step_through_orchestrator() {
    let socket = test_socket("workflow");
    let orchestrator = SwarmOrchestrator::bind(Some(socket.clone())).await.unwrap();
    let orch = Arc::new(TokioMutex::new(orchestrator));
    let accept_handle = SwarmOrchestrator::start_accept_loop(orch.clone());

    let mut general = GeneralAgent::new();
    general.connect(Some(socket.clone())).await.unwrap();
    let agent_handle = tokio::spawn(async move { general.run().await });
    let _agent_id = wait_for_agent(&orch, AgentType::General).await;

    let pool_mgr = Arc::new(TokioMutex::new(AgentPoolManager::new(Some(socket.clone()))));
    let mut engine = WorkflowEngine::new(pool_mgr).with_orchestrator(orch.clone());
    let workflow = Workflow {
        name: "integration-workflow".to_string(),
        description: "verify real workflow dispatch".to_string(),
        timeout_seconds: 5,
        steps: vec![WorkflowStep {
            id: "record".to_string(),
            name: "Record Step".to_string(),
            mode: ExecutionMode::Serial,
            depends_on: vec![],
            task: "record workflow integration task".to_string(),
            condition: None,
            timeout_seconds: 5,
            retry_count: 0,
        }],
    };

    let state = engine.execute(&workflow).await.unwrap();
    assert_eq!(state.status, WorkflowStatus::Completed);
    let step = state.step_results.get("record").unwrap();
    assert!(step.output.as_ref().unwrap().contains("success"));

    agent_handle.abort();
    accept_handle.abort();
    let _ = tokio::fs::remove_file(socket).await;
}
