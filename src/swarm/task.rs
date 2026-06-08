// src/swarm/task.rs
// 📋 蜂群任务模型 — SwarmTask / TaskResult / TaskStatus / TaskPriority
//
// 设计文档: docs/analyses/swarm-architecture-gaps-analysis/04-code-paths-implementation.md

use serde::{Deserialize, Serialize};

/// 蜂群任务 — 可派发给任意 Agent 执行的工作单元
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    /// 任务唯一 ID
    pub task_id: String,
    /// 任务类型（如 "code_search", "code_review", "memory_extract"）
    pub task_type: String,
    /// 目标 Agent 类型
    pub target_agent_type: String,
    /// 任务负载（JSON 格式的任务参数）
    pub payload: serde_json::Value,
    /// 优先级
    pub priority: TaskPriority,
    /// 超时秒数
    pub timeout_seconds: u64,
    /// 最大重试次数
    pub max_retries: u32,
    /// 当前状态
    pub status: TaskStatus,
    /// 创建时间戳（Unix 秒）
    pub created_at: u64,
    /// 执行 Agent ID
    pub agent_id: Option<String>,
    /// 执行结果
    pub result: Option<TaskResult>,
}

impl SwarmTask {
    /// 创建一个新任务
    pub fn new(task_type: impl Into<String>, payload: serde_json::Value) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            task_id: Self::generate_id(),
            task_type: task_type.into(),
            target_agent_type: String::new(),
            payload,
            priority: TaskPriority::Normal,
            timeout_seconds: 60,
            max_retries: 0,
            status: TaskStatus::Pending,
            created_at: now,
            agent_id: None,
            result: None,
        }
    }

    /// 设置目标 Agent 类型
    pub fn with_target(mut self, agent_type: impl Into<String>) -> Self {
        self.target_agent_type = agent_type.into();
        self
    }

    /// 设置优先级
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// 设置超时
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_seconds = seconds;
        self
    }

    /// 设置最大重试次数
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// 设置执行 Agent ID
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// 从 JSON-RPC params 中解析任务，兼容旧的 {task, params} 形态。
    pub fn from_rpc_params(params: Option<&serde_json::Value>) -> Result<Self, String> {
        let Some(params) = params else {
            return Err("dispatch_task params is required".to_string());
        };

        if let Some(task_value) = params.get("task") {
            if task_value.is_object() {
                return serde_json::from_value::<SwarmTask>(task_value.clone())
                    .map_err(|e| format!("invalid SwarmTask: {}", e));
            }
        }

        let task_description = params
            .get("task")
            .or_else(|| params.get("task_description"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if task_description.trim().is_empty() {
            return Err("task description is empty".to_string());
        }

        let task_params = params
            .get("params")
            .or_else(|| params.get("task_params"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let target = params
            .get("agent_type")
            .or_else(|| params.get("target_agent_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("general");
        let timeout_seconds = params
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(60);

        Ok(Self::new(
            "dispatch_task",
            serde_json::json!({
                "task_description": task_description,
                "task_params": task_params,
            }),
        )
        .with_target(target)
        .with_timeout(timeout_seconds))
    }

    /// 生成 JSON-RPC params 载荷。
    pub fn to_rpc_params(&self) -> serde_json::Value {
        serde_json::json!({ "task": self })
    }

    /// 任务描述文本。
    pub fn description(&self) -> String {
        self.payload
            .get("task_description")
            .or_else(|| self.payload.get("task"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// 任务参数 JSON。
    pub fn params(&self) -> serde_json::Value {
        self.payload
            .get("task_params")
            .or_else(|| self.payload.get("params"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}))
    }

    /// 生成任务 ID
    fn generate_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format!("task-{:016x}", now.as_nanos())
    }
}

/// 任务执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// 任务 ID
    pub task_id: String,
    /// 执行状态
    pub status: TaskStatus,
    /// 结果数据
    pub data: Option<serde_json::Value>,
    /// 错误信息
    pub error: Option<String>,
    /// 开始时间戳
    pub started_at: u64,
    /// 完成时间戳
    pub completed_at: u64,
}

impl TaskResult {
    /// 创建成功结果
    pub fn success(task_id: impl Into<String>, data: serde_json::Value) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            task_id: task_id.into(),
            status: TaskStatus::Completed,
            data: Some(data),
            error: None,
            started_at: now,
            completed_at: now,
        }
    }

    /// 创建失败结果
    pub fn failed(task_id: impl Into<String>, error: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            task_id: task_id.into(),
            status: TaskStatus::Failed,
            data: None,
            error: Some(error.into()),
            started_at: now,
            completed_at: now,
        }
    }
}

/// 任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 等待执行
    Pending,
    /// 正在执行
    Running,
    /// 成功完成
    Completed,
    /// 执行失败
    Failed,
    /// 已取消
    Cancelled,
    /// 超时
    TimedOut,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::TimedOut => "timed_out",
        }
    }
}

/// 任务优先级
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Normal,
    High,
    Critical,
}

impl TaskPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskPriority::Low => "low",
            TaskPriority::Normal => "normal",
            TaskPriority::High => "high",
            TaskPriority::Critical => "critical",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swarm_task_creation() {
        let task = SwarmTask::new(
            "code_search",
            serde_json::json!({ "pattern": "fn main", "path": "./src" }),
        );
        assert_eq!(task.task_type, "code_search");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.priority, TaskPriority::Normal);
        assert!(task.task_id.starts_with("task-"));
    }

    #[test]
    fn test_task_result_success() {
        let result = TaskResult::success("task-1", serde_json::json!({ "matches": 42 }));
        assert_eq!(result.status, TaskStatus::Completed);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_task_result_failed() {
        let result = TaskResult::failed("task-2", "agent not found");
        assert_eq!(result.status, TaskStatus::Failed);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_task_serialization_roundtrip() {
        let task = SwarmTask::new("test", serde_json::json!({ "key": "value" }));
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: SwarmTask = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_type, task.task_type);
        assert_eq!(deserialized.status, task.status);
    }
}
