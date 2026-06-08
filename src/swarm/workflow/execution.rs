use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Mutex as TokioMutex;

use crate::swarm::pool::AgentPoolManager;

pub(super) async fn execute_step(
    pool_manager: Arc<TokioMutex<AgentPoolManager>>,
    _task: &str,
    step_name: String,
) -> Result<String> {
    let (instance_id, instance_name) = {
        let mut mgr = pool_manager.lock().await;
        let pool = &mut mgr.general_pool;
        if let Some(instance) = pool.acquire().await {
            let id = instance.id.clone();
            let name = format!("{}_v{}", id, step_name);
            (id, name)
        } else {
            return Err(anyhow::anyhow!("无可用 Agent 实例执行步骤 '{}'", step_name));
        }
    };

    eprintln!(
        "📋 [Workflow] 步骤 '{}' 使用实例 '{}'",
        step_name, instance_id
    );
    // 模拟执行（真实场景中通过 UDS 发送任务给 Agent）
    tokio::time::sleep(Duration::from_millis(200)).await;
    let result = format!(
        "步骤 '{}' 执行完成，使用了实例 '{}'",
        step_name, instance_name
    );

    // 释放实例回池
    {
        let mut mgr = pool_manager.lock().await;
        mgr.general_pool.release(&instance_id).await.ok();
    }

    Ok(result)
}
