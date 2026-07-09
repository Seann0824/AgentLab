use std::fmt;

/// 桌面端应用错误类型
#[derive(Debug)]
pub enum AppError {
    EnvVarMissing { name: &'static str },
    SessionNotFound,
    AgentRunError(String),
    SendEventError(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::EnvVarMissing { name } => write!(f, "missing environment variable: {}", name),
            AppError::SessionNotFound => write!(f, "session not found"),
            AppError::AgentRunError(msg) => write!(f, "agent run error: {}", msg),
            AppError::SendEventError(msg) => write!(f, "send event error: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        err.to_string()
    }
}
