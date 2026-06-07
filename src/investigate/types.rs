/// 错误排查 — 数据类型
///
/// 工具调用报错时自动保存的「错误现场」快照

use serde::{Deserialize, Serialize};

use crate::session::types::SerializableMessage;

/// 错误快照 — 工具调用报错时自动保存的「错误现场」
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSnapshot {
    /// 快照 ID（时间戳，如 "20250608_103000"）
    pub id: String,
    /// 创建时间（ISO 格式）
    pub created_at: String,
    /// 错误信息
    pub error: ErrorInfo,
    /// 上下文消息（最后几轮关键消息）
    pub context: Vec<SerializableMessage>,
    /// 当时的任务状态
    pub task_context: TaskContextSnapshot,
}

/// 错误信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// 出错的工具名称
    pub tool_name: String,
    /// 工具调用参数
    pub args: serde_json::Value,
    /// 错误输出（stdout + stderr 合并）
    pub output: String,
    /// 退出码（如果是 shell 命令）
    pub exit_code: Option<i32>,
    /// 执行耗时（毫秒）
    pub duration_ms: u64,
}

/// 任务上下文快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContextSnapshot {
    /// PLAN.md 内容（如果有）
    pub plan: Option<String>,
    /// AGENDA.md 内容（如果有）
    pub agenda: Option<String>,
    /// 当前轮次
    pub turn: usize,
    /// 总消息数
    pub total_messages: usize,
}

/// 快照摘要（用于列表展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub id: String,
    pub created_at: String,
    pub tool_name: String,
    pub error_preview: String,
}
