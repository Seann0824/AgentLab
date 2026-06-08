use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tokio::sync::Mutex as TokioMutex;

use super::execution::execute_step;
use super::time::format_now;
use super::types::{
    Condition, ConditionType, ExecutionMode, StepResult, StepStatus, Workflow, WorkflowState,
    WorkflowStatus, WorkflowStep,
};
use crate::swarm::orchestrator::SwarmOrchestrator;
use crate::swarm::pool::AgentPoolManager;

pub struct WorkflowEngine {
    /// Agent 池管理器
    pool_manager: Arc<TokioMutex<AgentPoolManager>>,
    /// 可选 Orchestrator，用于真实派发 Workflow 步骤。
    orchestrator: Option<Arc<TokioMutex<SwarmOrchestrator>>>,
    /// 活跃的 Workflow 状态
    active_workflows: Arc<TokioMutex<HashMap<String, WorkflowState>>>,
}

impl WorkflowEngine {
    /// 创建新的 Workflow 引擎
    pub fn new(pool_manager: Arc<TokioMutex<AgentPoolManager>>) -> Self {
        Self {
            pool_manager,
            orchestrator: None,
            active_workflows: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    /// 注入 SwarmOrchestrator，使 Workflow 步骤通过真实 dispatch_task 执行。
    pub fn with_orchestrator(mut self, orchestrator: Arc<TokioMutex<SwarmOrchestrator>>) -> Self {
        self.orchestrator = Some(orchestrator);
        self
    }

    /// 执行 Workflow
    pub async fn execute(&mut self, workflow: &Workflow) -> Result<WorkflowState> {
        let execution_id = format!(
            "wf-{}-{}",
            workflow.name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        eprintln!(
            "📋 [Workflow] 开始执行: {} (id: {})",
            workflow.name, execution_id
        );

        // 初始化 Workflow 状态
        let mut state = WorkflowState {
            workflow_name: workflow.name.clone(),
            execution_id: execution_id.clone(),
            status: WorkflowStatus::Running,
            step_results: HashMap::new(),
            started_at: format_now(),
            completed_at: None,
            error: None,
        };

        // 初始化所有步骤为 Pending
        for step in &workflow.steps {
            state.step_results.insert(
                step.id.clone(),
                StepResult {
                    step_id: step.id.clone(),
                    step_name: step.name.clone(),
                    status: StepStatus::Pending,
                    output: None,
                    error: None,
                    started_at: String::new(),
                    completed_at: None,
                    duration_ms: 0,
                },
            );
        }

        // 保存到活跃列表
        {
            let mut active = self.active_workflows.lock().await;
            active.insert(execution_id.clone(), state.clone());
        }

        // 确定执行顺序（拓扑排序）
        let execution_order = self.topological_sort(&workflow.steps)?;

        for group in &execution_order {
            // 每组内可以并行执行
            let mut handles = Vec::new();

            for step_id in group {
                if let Some(step) = workflow.steps.iter().find(|s| s.id == *step_id) {
                    // 检查条件
                    if step.mode == ExecutionMode::Conditional {
                        if let Some(ref condition) = step.condition {
                            if !self.evaluate_condition(condition, &state).await {
                                // 条件不满足，跳过
                                if let Some(step_state) = state.step_results.get_mut(step_id) {
                                    step_state.status = StepStatus::Skipped;
                                    step_state.started_at = format_now();
                                    step_state.completed_at = Some(format_now());
                                }
                                eprintln!("📋 [Workflow] 步骤 '{}' 条件不满足，跳过", step.name);
                                continue;
                            }
                        }
                    }

                    // 更新状态为 Running
                    if let Some(step_state) = state.step_results.get_mut(step_id) {
                        step_state.status = StepStatus::Running;
                        step_state.started_at = format_now();
                    }

                    let pool_mgr = self.pool_manager.clone();
                    let orchestrator = self.orchestrator.clone();
                    let task_desc = step.task.clone();
                    let step_name = step.name.clone();
                    let timeout_seconds = if step.timeout_seconds == 0 {
                        workflow.timeout_seconds
                    } else {
                        step.timeout_seconds
                    };
                    let retry_count = step.retry_count;

                    // 启动并行执行
                    let handle = tokio::spawn(async move {
                        execute_step(
                            pool_mgr,
                            orchestrator,
                            &task_desc,
                            step_name,
                            timeout_seconds,
                            retry_count,
                        )
                        .await
                    });
                    handles.push((step_id.clone(), handle, step.name.clone()));
                }
            }

            // 等待本组所有步骤完成
            for (step_id, handle, sname) in handles {
                match handle.await {
                    Ok(result) => {
                        if let Some(step_state) = state.step_results.get_mut(&step_id) {
                            match result {
                                Ok(output) => {
                                    step_state.status = StepStatus::Success;
                                    step_state.output = Some(output);
                                    step_state.completed_at = Some(format_now());
                                    eprintln!("📋 [Workflow] 步骤 '{}' ✅ 成功", sname);
                                }
                                Err(e) => {
                                    step_state.status = StepStatus::Failed;
                                    step_state.error = Some(e.to_string());
                                    step_state.completed_at = Some(format_now());
                                    eprintln!("📋 [Workflow] 步骤 '{}' ❌ 失败: {}", sname, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("📋 [Workflow] 步骤 '{}' 执行异常: {}", sname, e);
                    }
                }
            }
        }

        // 更新最终状态
        let failed_count = state
            .step_results
            .values()
            .filter(|r| r.status == StepStatus::Failed)
            .count();
        let success_count = state
            .step_results
            .values()
            .filter(|r| r.status == StepStatus::Success)
            .count();

        state.status = if failed_count == 0 && success_count > 0 {
            WorkflowStatus::Completed
        } else if failed_count > 0 && success_count > 0 {
            WorkflowStatus::PartialFailed
        } else if failed_count > 0 {
            WorkflowStatus::Failed
        } else {
            WorkflowStatus::Completed
        };
        state.completed_at = Some(format_now());

        // 更新活跃列表
        {
            let mut active = self.active_workflows.lock().await;
            active.insert(execution_id, state.clone());
        }

        eprintln!(
            "📋 [Workflow] 执行完成: {} (成功: {}, 失败: {})",
            workflow.name, success_count, failed_count
        );

        Ok(state)
    }

    /// 拓扑排序，返回并行执行的分组
    pub(super) fn topological_sort(&self, steps: &[WorkflowStep]) -> Result<Vec<Vec<String>>> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for step in steps {
            in_degree.entry(&step.id).or_insert(0);
            adjacency.entry(&step.id).or_insert(Vec::new());
        }

        for step in steps {
            for dep in &step.depends_on {
                if let Some(children) = adjacency.get_mut(dep.as_str()) {
                    children.push(&step.id);
                }
                *in_degree.entry(&step.id).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(id, _)| *id)
            .collect();

        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut visited = 0usize;

        while !queue.is_empty() {
            let current_group: Vec<String> = queue.iter().map(|s| s.to_string()).collect();
            groups.push(current_group);

            let mut next_queue = Vec::new();

            for node in &queue {
                visited += 1;
                if let Some(children) = adjacency.get(node) {
                    for child in children {
                        if let Some(deg) = in_degree.get_mut(child) {
                            *deg -= 1;
                            if *deg == 0 {
                                next_queue.push(*child);
                            }
                        }
                    }
                }
            }

            queue = next_queue;
        }

        if visited != steps.len() {
            anyhow::bail!("Workflow 步骤中存在循环依赖");
        }

        Ok(groups)
    }

    /// 评估条件
    async fn evaluate_condition(&self, condition: &Condition, state: &WorkflowState) -> bool {
        match condition.condition_type {
            ConditionType::Success => state
                .step_results
                .values()
                .all(|r| r.status == StepStatus::Success),
            ConditionType::Failure => state
                .step_results
                .values()
                .any(|r| r.status == StepStatus::Failed),
            ConditionType::OutputContains => state
                .step_results
                .values()
                .filter_map(|r| r.output.as_ref())
                .any(|out| out.contains(&condition.value)),
            ConditionType::OutputEquals => state
                .step_results
                .values()
                .filter_map(|r| r.output.as_ref())
                .any(|out| out == &condition.value),
        }
    }

    /// 获取 Workflow 执行状态
    pub async fn get_state(&self, execution_id: &str) -> Option<WorkflowState> {
        let active = self.active_workflows.lock().await;
        active.get(execution_id).cloned()
    }

    /// 列出所有活跃的 Workflow
    pub async fn list_active(&self) -> Vec<WorkflowState> {
        let active = self.active_workflows.lock().await;
        active.values().cloned().collect()
    }

    /// 取消 Workflow
    pub async fn cancel(&self, execution_id: &str) -> bool {
        let mut active = self.active_workflows.lock().await;
        if let Some(state) = active.get_mut(execution_id) {
            if state.status == WorkflowStatus::Running || state.status == WorkflowStatus::Pending {
                state.status = WorkflowStatus::Cancelled;
                state.completed_at = Some(format_now());
                eprintln!("📋 [Workflow] 已取消: {}", execution_id);
                return true;
            }
        }
        false
    }
}
