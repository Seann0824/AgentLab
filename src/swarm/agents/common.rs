use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use crate::swarm::task::{SwarmTask, TaskResult};
use crate::swarm::transport::UdsClient;

pub async fn send_task_result(
    client: &Option<Arc<TokioMutex<UdsClient>>>,
    request_id: &str,
    task_result: TaskResult,
) {
    if let Some(client_arc) = client {
        let mut client = client_arc.lock().await;
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "task_result": task_result,
            }
        });
        let _ = client
            .send_raw(&serde_json::to_string(&resp).unwrap())
            .await;
    }
}

pub fn task_success(task: &SwarmTask, data: serde_json::Value) -> TaskResult {
    TaskResult::success(task.task_id.clone(), data)
}

pub fn task_failed(task_id: impl Into<String>, error: impl Into<String>) -> TaskResult {
    TaskResult::failed(task_id, error)
}
