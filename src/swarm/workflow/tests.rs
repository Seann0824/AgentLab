use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use super::*;
use crate::swarm::pool::AgentPoolManager;

#[test]
fn test_topological_sort() {
    let steps = vec![
        WorkflowStep {
            id: "step1".to_string(),
            name: "Step 1".to_string(),
            mode: ExecutionMode::Serial,
            depends_on: vec![],
            task: "task1".to_string(),
            condition: None,
            timeout_seconds: 0,
            retry_count: 0,
        },
        WorkflowStep {
            id: "step2a".to_string(),
            name: "Step 2A".to_string(),
            mode: ExecutionMode::Parallel,
            depends_on: vec!["step1".to_string()],
            task: "task2a".to_string(),
            condition: None,
            timeout_seconds: 0,
            retry_count: 0,
        },
        WorkflowStep {
            id: "step2b".to_string(),
            name: "Step 2B".to_string(),
            mode: ExecutionMode::Parallel,
            depends_on: vec!["step1".to_string()],
            task: "task2b".to_string(),
            condition: None,
            timeout_seconds: 0,
            retry_count: 0,
        },
        WorkflowStep {
            id: "step3".to_string(),
            name: "Step 3".to_string(),
            mode: ExecutionMode::Serial,
            depends_on: vec!["step2a".to_string(), "step2b".to_string()],
            task: "task3".to_string(),
            condition: None,
            timeout_seconds: 0,
            retry_count: 0,
        },
    ];

    let pool_mgr = Arc::new(TokioMutex::new(AgentPoolManager::new(None)));
    let engine = WorkflowEngine::new(pool_mgr);

    let groups = engine.topological_sort(&steps).unwrap();
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].len(), 1); // step1
    assert_eq!(groups[1].len(), 2); // step2a, step2b
    assert_eq!(groups[2].len(), 1); // step3
}
