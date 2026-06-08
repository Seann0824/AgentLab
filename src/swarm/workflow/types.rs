use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
