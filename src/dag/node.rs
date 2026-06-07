// src/dag/node.rs
// 节点定义 — DAG 中的任务节点

use crate::dag::types::{
    InputMode, NodeLog, NodeStatus, OutputMode, ReviewCriteria, ReviewResult,
};

/// 节点定义（DAG 中的一个任务节点）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeDef {
    /// 节点唯一标识
    pub id: String,
    /// 节点名称
    pub name: String,
    /// 节点描述（作为 Worker 的系统提示）
    pub description: String,
    /// Worker 的详细指令
    pub worker_instruction: String,
    /// Reviewer 的审核标准
    pub review_criteria: ReviewCriteria,
    /// 输入模式
    pub input_mode: InputMode,
    /// 输出模式
    pub output_mode: OutputMode,
    /// 节点标签（用于分类和过滤）
    pub tags: Vec<String>,
}

impl NodeDef {
    /// 创建新的节点定义
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            worker_instruction: String::new(),
            review_criteria: ReviewCriteria::new(),
            input_mode: InputMode::Merged,
            output_mode: OutputMode::Text,
            tags: Vec::new(),
        }
    }

    /// 设置节点描述
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// 设置 Worker 指令
    pub fn worker_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.worker_instruction = instruction.into();
        self
    }

    /// 设置审核标准
    pub fn review_criteria(mut self, criteria: ReviewCriteria) -> Self {
        self.review_criteria = criteria;
        self
    }

    /// 设置输入模式
    pub fn input_mode(mut self, mode: InputMode) -> Self {
        self.input_mode = mode;
        self
    }

    /// 设置输出模式
    pub fn output_mode(mut self, mode: OutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// 添加标签
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }
}

/// 节点运行时实例
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeInstance {
    /// 对应 NodeDef.id
    pub node_id: String,
    /// 当前状态
    pub status: NodeStatus,
    /// 接收到的输入数据
    pub input: Option<serde_json::Value>,
    /// Worker 产生的输出
    pub worker_output: Option<String>,
    /// 审核结果
    pub review_result: Option<ReviewResult>,
    /// 最终输出（审核通过后的转发数据）
    pub final_output: Option<serde_json::Value>,
    /// 执行日志
    pub logs: Vec<NodeLog>,
    /// 开始时间（Unix 时间戳）
    pub started_at: Option<f64>,
    /// 完成时间（Unix 时间戳）
    pub completed_at: Option<f64>,
    /// 重试次数
    pub retry_count: u32,
}

impl NodeInstance {
    /// 创建新的节点运行时实例
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            status: NodeStatus::Pending,
            input: None,
            worker_output: None,
            review_result: None,
            final_output: None,
            logs: Vec::new(),
            started_at: None,
            completed_at: None,
            retry_count: 0,
        }
    }

    /// 是否已完成
    pub fn is_completed(&self) -> bool {
        self.status == NodeStatus::Completed
    }

    /// 是否处于可执行状态（Ready 或 Rejected 后重试）
    pub fn is_executable(&self) -> bool {
        matches!(self.status, NodeStatus::Ready)
            || matches!(self.status, NodeStatus::Rejected { .. })
    }

    /// 更新状态并记录日志
    pub fn transition_to(&mut self, new_status: NodeStatus) {
        let old_status = std::mem::replace(&mut self.status, new_status.clone());

        // 记录开始/完成时间
        if matches!(self.status, NodeStatus::Working) && self.started_at.is_none() {
            self.started_at = Some(crate::dag::utils::now_secs());
        }
        if self.status.is_terminal() {
            self.completed_at = Some(crate::dag::utils::now_secs());
        }

        self.logs.push(NodeLog {
            timestamp: chrono_now(),
            level: crate::dag::types::LogLevel::Info,
            source: crate::dag::types::LogSource::Engine,
            message: format!(
                "状态变更: {:?} → {:?}",
                old_status, new_status
            ),
            metadata: None,
        });
    }
}

/// 获取当前时间的字符串表示
fn chrono_now() -> String {
    // 使用简单的 Unix 时间戳字符串
    format!("{:.3}", crate::dag::utils::now_secs())
}
