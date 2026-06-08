/// Agent 运行事件类型 — 自我迭代、回放和观测的统一事件模型。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    ModelRequest,
    ModelResponse,
    ModelError,
    ToolStarted,
    ToolFinished,
    ToolFailed,
    SwarmDispatchStarted,
    SwarmDispatchFinished,
    WorkflowStepStarted,
    WorkflowStepFinished,
    GoalUpdated,
}

/// 一条结构化运行事件。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunEvent {
    pub event_id: String,
    pub kind: RunEventKind,
    pub timestamp_ms: u128,
    pub subject: String,
    pub attributes: serde_json::Value,
}

impl RunEvent {
    pub fn new(
        kind: RunEventKind,
        subject: impl Into<String>,
        attributes: serde_json::Value,
    ) -> Self {
        let timestamp_ms = now_ms();
        Self {
            event_id: format!("evt-{:x}", timestamp_ms),
            kind,
            timestamp_ms,
            subject: subject.into(),
            attributes,
        }
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
