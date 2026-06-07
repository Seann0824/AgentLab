// src/dag/types.rs
// DAG 任务编排系统 — 核心类型定义

use std::collections::HashMap;

// =====================================================================
// 节点运行时状态
// =====================================================================

/// 节点运行时状态
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum NodeStatus {
    /// 等待依赖就绪
    Pending,
    /// 依赖已就绪，等待调度
    Ready,
    /// Worker 正在执行
    Working,
    /// Reviewer 正在审核
    Reviewing,
    /// 审核通过
    Approved,
    /// 审核不通过，需要重试
    Rejected { retry_count: u32, reason: String },
    /// 已完成（审核通过且输出已转发）
    Completed,
    /// 执行失败（不可恢复错误）
    Failed { error: String },
    /// 跳过（配置为失败时跳过）
    Skipped { reason: String },
}

impl NodeStatus {
    /// 是否为终态
    pub fn is_terminal(&self) -> bool {
        matches!(self, NodeStatus::Completed | NodeStatus::Failed { .. } | NodeStatus::Skipped { .. })
    }

    /// 是否为失败态
    pub fn is_failed(&self) -> bool {
        matches!(self, NodeStatus::Failed { .. } | NodeStatus::Skipped { .. })
    }
}

// =====================================================================
// Pipeline 执行状态
// =====================================================================

/// Pipeline 执行状态
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PipelineStatus {
    /// 等待执行
    Pending,
    /// 执行中
    Running,
    /// 全部节点已完成
    Completed,
    /// 部分节点失败
    Failed { failed_nodes: Vec<String> },
    /// 已取消
    Cancelled,
}

// =====================================================================
// 审核相关类型
// =====================================================================

/// 审核标准
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewCriteria {
    /// 审核清单（逐条检查）
    pub check_items: Vec<String>,
    /// 审核指南
    pub guidelines: String,
}

impl ReviewCriteria {
    pub fn new() -> Self {
        Self {
            check_items: Vec::new(),
            guidelines: String::new(),
        }
    }

    /// 添加一条检查项
    pub fn check(mut self, item: impl Into<String>) -> Self {
        self.check_items.push(item.into());
        self
    }

    /// 设置审核指南
    pub fn guidelines(mut self, guidelines: impl Into<String>) -> Self {
        self.guidelines = guidelines.into();
        self
    }
}

impl Default for ReviewCriteria {
    fn default() -> Self {
        Self::new()
    }
}

/// 逐项检查结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckResult {
    pub item: String,
    pub passed: bool,
    pub comment: String,
}

/// 审核结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewResult {
    pub passed: bool,
    pub score: Option<f32>,
    pub feedback: String,
    pub details: Vec<CheckResult>,
}

// =====================================================================
// 节点输入/输出模式
// =====================================================================

/// 输入模式
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InputMode {
    /// 接收所有上游输出合并后的数据
    Merged,
    /// 选择特定上游字段
    Select { from_node: String, fields: Vec<String> },
    /// 接收原始用户输入
    RawInput,
}

/// 输出模式
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum OutputMode {
    /// 原始文本输出
    Text,
    /// 结构化 JSON 输出
    Json { schema: Option<serde_json::Value> },
    /// 文件输出
    File { path_pattern: String },
}

// =====================================================================
// 审核模式
// =====================================================================

/// 审核模式
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ReviewMode {
    /// 逐项检查（按 check_items 列表逐一审核）
    Checklist,
    /// 自由评估（给出整体评分和反馈）
    FreeForm,
    /// 对比审核（与预期结果对比）
    Comparison { expected: serde_json::Value },
}

// =====================================================================
// 数据合并策略
// =====================================================================

/// 数据合并策略
#[derive(Debug, Clone)]
pub enum MergeStrategy {
    /// 将所有上游输出合并为一个 JSON 对象（按节点 ID 分字段）
    ByNodeId,
    /// 将所有上游输出合并为一个数组
    Array,
    /// 使用自定义合并函数（预留）
    Custom { merge_fn: String },
}

// =====================================================================
// Worker 与 Reviewer 的执行结果
// =====================================================================

/// Worker 执行结果
#[derive(Debug, Clone)]
pub struct WorkerOutput {
    /// 原始输出内容
    pub content: String,
    /// 结构化输出（如果有定义 schema）
    pub structured: Option<serde_json::Value>,
    /// 执行日志
    pub execution_log: Vec<String>,
    /// 耗时（秒）
    pub duration_secs: f64,
}

/// 审核输出
#[derive(Debug, Clone)]
pub struct ReviewOutput {
    pub passed: bool,
    pub score: f32,
    pub feedback: String,
    pub check_results: Vec<CheckResult>,
    pub suggestions: Vec<String>,
}

// =====================================================================
// 节点执行结果（内部使用）
// =====================================================================

/// 节点执行结果（内部协调用）
#[derive(Debug, Clone)]
pub enum NodeResult {
    /// 执行成功（审核通过）
    Success {
        output: String,
        review: ReviewOutput,
    },
    /// 需要修订（审核不通过）
    NeedsRevision {
        worker_output: WorkerOutput,
        review: ReviewOutput,
    },
    /// 重试耗尽后失败
    FailedAfterRetries {
        last_worker_output: WorkerOutput,
        last_review: ReviewOutput,
        retries: u32,
    },
}

// =====================================================================
// 日志与事件类型
// =====================================================================

/// 日志级别
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

/// 日志来源
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LogSource {
    Engine,
    Worker,
    Reviewer,
    DataFlow,
}

/// 节点执行过程中的日志条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeLog {
    pub timestamp: String,
    pub level: LogLevel,
    pub source: LogSource,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
}

/// DAG 运行时事件
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DAGEvent {
    /// Pipeline 开始执行
    PipelineStarted { id: String, total_nodes: usize },
    /// 节点状态变更
    NodeStatusChanged { node_id: String, old_status: NodeStatus, new_status: NodeStatus },
    /// Worker 开始执行
    WorkerStarted { node_id: String },
    /// Worker 完成
    WorkerCompleted { node_id: String, duration_secs: f64 },
    /// Reviewer 开始审核
    ReviewerStarted { node_id: String },
    /// 审核完成
    ReviewCompleted { node_id: String, passed: bool, score: f32 },
    /// 节点重试
    NodeRetrying { node_id: String, attempt: u32, reason: String },
    /// Pipeline 完成
    PipelineCompleted { id: String, total_duration_secs: f64, node_count: usize },
    /// Pipeline 失败
    PipelineFailed { id: String, error: String, failed_node: String },
}

// =====================================================================
// 错误类型
// =====================================================================

/// DAG 错误
#[derive(Debug, Clone)]
pub enum DAGError {
    /// DAG 中存在环
    CycleDetected,
    /// 节点未定义
    NodeNotFound(String),
    /// 边的节点未定义
    EdgeNodeNotFound(String, String),
    /// Pipeline 执行超时
    ExecutionTimeout,
    /// 节点执行超时
    NodeTimeout(String),
    /// 节点执行失败
    NodeExecutionFailed(String, String),
    /// 内部错误
    Internal(String),
}

impl std::fmt::Display for DAGError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DAGError::CycleDetected => write!(f, "DAG 中存在环，无法进行拓扑排序"),
            DAGError::NodeNotFound(id) => write!(f, "节点 '{}' 未定义", id),
            DAGError::EdgeNodeNotFound(from, to) => write!(f, "边 '{}' -> '{}' 引用了未定义的节点", from, to),
            DAGError::ExecutionTimeout => write!(f, "Pipeline 执行超时"),
            DAGError::NodeTimeout(id) => write!(f, "节点 '{}' 执行超时", id),
            DAGError::NodeExecutionFailed(id, msg) => write!(f, "节点 '{}' 执行失败: {}", id, msg),
            DAGError::Internal(msg) => write!(f, "内部错误: {}", msg),
        }
    }
}

impl std::error::Error for DAGError {}

pub type DAGResult<T> = Result<T, DAGError>;
