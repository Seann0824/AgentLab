// src/swarm/workflow.rs
// 📋 Workflow Engine — 任务编排引擎
//
// 支持串行/并行/条件分支的任务编排执行。
// 依赖 Phase 3 的 Agent Pool 来分配执行 Agent。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as TokioMutex;

use super::pool::AgentPoolManager;

// ─── 数据类型 ─────────────────────────────────────────

/// Workflow 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Workflow 名称
    pub name: String,
    /// 描述
    pub description: String,
    /// 步骤列表
    pub steps: Vec<WorkflowStep>,
    /// 全局超时（秒）
    pub timeout_seconds: u64,
}

/// Workflow 步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// 步骤 ID（唯一标识）
    pub id: String,
    /// 步骤名称
    pub name: String,
    /// 执行模式
    pub mode: ExecutionMode,
    /// 依赖步骤 ID 列表
    pub depends_on: Vec<String>,
    /// 任务描述（传递给 Agent 执行）
    pub task: String,
    /// 条件分支（可选）
    pub condition: Option<Condition>,
    /// 超时（秒），0 表示使用全局超时
    pub timeout_seconds: u64,
    /// 重试次数
    pub retry_count: u32,
}

/// 执行模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionMode {
    /// 串行执行
    Serial,
    /// 并行执行
    Parallel,
    /// 条件执行（满足条件才执行）
    Conditional,
}

/// 条件分支
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// 条件类型
    pub condition_type: ConditionType,
    /// 条件值（如依赖步骤的输出包含此值）
    pub value: String,
    /// 条件不满足时的替代步骤 ID
    pub else_step: Option<String>,
}

/// 条件类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionType {
    /// 依赖步骤输出包含指定值
    OutputContains,
    /// 依赖步骤输出等于指定值
    OutputEquals,
    /// 依赖步骤成功
    Success,
    /// 依赖步骤失败
    Failure,
}

/// Workflow 执行状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    /// Workflow 名称
    pub workflow_name: String,
    /// 执行 ID
    pub execution_id: String,
    /// 状态
    pub status: WorkflowStatus,
    /// 各步骤执行结果
    pub step_results: HashMap<String, StepResult>,
    /// 开始时间
    pub started_at: String,
    /// 结束时间
    pub completed_at: Option<String>,
    /// 错误信息
    pub error: Option<String>,
}

/// Workflow 状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowStatus {
    /// 等待执行
    Pending,
    /// 正在执行
    Running,
    /// 全部完成
    Completed,
    /// 部分失败
    PartialFailed,
    /// 全部失败
    Failed,
    /// 已取消
    Cancelled,
    /// 超时
    TimedOut,
}

/// 步骤执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// 步骤 ID
    pub step_id: String,
    /// 步骤名称
    pub step_name: String,
    /// 状态
    pub status: StepStatus,
    /// 输出
    pub output: Option<String>,
    /// 错误信息
    pub error: Option<String>,
    /// 开始时间
    pub started_at: String,
    /// 结束时间
    pub completed_at: Option<String>,
    /// 执行耗时（毫秒）
    pub duration_ms: u64,
}

/// 步骤状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    /// 等待执行
    Pending,
    /// 正在执行
    Running,
    /// 成功完成
    Success,
    /// 失败
    Failed,
    /// 跳过（条件不满足）
    Skipped,
    /// 取消
    Cancelled,
}

// ─── Workflow Engine ───────────────────────────────────

/// Workflow 执行引擎
pub struct WorkflowEngine {
    /// Agent 池管理器
    pool_manager: Arc<TokioMutex<AgentPoolManager>>,
    /// 活跃的 Workflow 状态
    active_workflows: Arc<TokioMutex<HashMap<String, WorkflowState>>>,
}

impl WorkflowEngine {
    /// 创建新的 Workflow 引擎
    pub fn new(pool_manager: Arc<TokioMutex<AgentPoolManager>>) -> Self {
        Self {
            pool_manager,
            active_workflows: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    /// 执行 Workflow
    pub async fn execute(&mut self, workflow: &Workflow) -> Result<WorkflowState> {
        let execution_id = format!("wf-{}-{}", workflow.name, 
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs());
        eprintln!("📋 [Workflow] 开始执行: {} (id: {})", workflow.name, execution_id);

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
            state.step_results.insert(step.id.clone(), StepResult {
                step_id: step.id.clone(),
                step_name: step.name.clone(),
                status: StepStatus::Pending,
                output: None,
                error: None,
                started_at: String::new(),
                completed_at: None,
                duration_ms: 0,
            });
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
                    let task_desc = step.task.clone();
                    let step_name = step.name.clone();

                    // 启动并行执行
                    let handle = tokio::spawn(async move {
                        execute_step(pool_mgr, &task_desc, step_name).await
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
        let failed_count = state.step_results.values()
            .filter(|r| r.status == StepStatus::Failed).count();
        let success_count = state.step_results.values()
            .filter(|r| r.status == StepStatus::Success).count();

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

        eprintln!("📋 [Workflow] 执行完成: {} (成功: {}, 失败: {})",
            workflow.name, success_count, failed_count);

        Ok(state)
    }

    /// 拓扑排序，返回并行执行的分组
    fn topological_sort(&self, steps: &[WorkflowStep]) -> Result<Vec<Vec<String>>> {
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

        let mut queue: Vec<&str> = in_degree.iter()
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
            ConditionType::Success => {
                state.step_results.values()
                    .all(|r| r.status == StepStatus::Success)
            }
            ConditionType::Failure => {
                state.step_results.values()
                    .any(|r| r.status == StepStatus::Failed)
            }
            ConditionType::OutputContains => {
                state.step_results.values()
                    .filter_map(|r| r.output.as_ref())
                    .any(|out| out.contains(&condition.value))
            }
            ConditionType::OutputEquals => {
                state.step_results.values()
                    .filter_map(|r| r.output.as_ref())
                    .any(|out| out == &condition.value)
            }
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

/// 在 Agent Pool 中执行一个步骤
async fn execute_step(
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

    eprintln!("📋 [Workflow] 步骤 '{}' 使用实例 '{}'", step_name, instance_id);
    // 模拟执行（真实场景中通过 UDS 发送任务给 Agent）
    tokio::time::sleep(Duration::from_millis(200)).await;
    let result = format!("步骤 '{}' 执行完成，使用了实例 '{}'", step_name, instance_name);

    // 释放实例回池
    {
        let mut mgr = pool_manager.lock().await;
        mgr.general_pool.release(&instance_id).await.ok();
    }

    Ok(result)
}

/// 获取当前时间字符串（ISO 8601 格式）
fn format_now() -> String {
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    // 转换为 ISO 8601 格式
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        1970 + (days / 365) as u32, // 近似年份
        ((days % 365) / 30 + 1) as u32, // 近似月份
        ((days % 365) % 30 + 1) as u32, // 近似日
        hours, minutes, seconds, millis)
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
