/// agent-lab-core 统一错误类型。
#[derive(Debug, thiserror::Error)]
pub enum AgentLabError {
    #[error("环境变量缺失: {name}")]
    EnvVarMissing { name: &'static str },

    #[error("存储错误: {0}")]
    Storage(#[from] crate::storage::StorageError),

    #[error("服务错误: {0}")]
    Service(#[from] crate::services::ServiceError),

    #[error("LLM 调用失败: {0}")]
    Llm(String),

    #[error("序列化/反序列化失败: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Provider 配置错误: {0}")]
    ProviderConfig(String),

    #[error("参数无效: {0}")]
    InvalidArgument(String),

    #[error("未知错误: {0}")]
    Unknown(String),
}

impl From<std::env::VarError> for AgentLabError {
    fn from(_: std::env::VarError) -> Self {
        // 注意：这里丢失了具体变量名，调用方应尽量使用 EnvVarMissing 明确指定。
        AgentLabError::EnvVarMissing {
            name: "UNKNOWN",
        }
    }
}

impl From<AgentLabError> for String {
    fn from(err: AgentLabError) -> Self {
        err.to_string()
    }
}
