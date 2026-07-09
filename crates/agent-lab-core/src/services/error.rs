/// 业务服务层统一错误类型。
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("存储错误: {0}")]
    Storage(#[from] crate::storage::StorageError),

    #[error("embedding 失败: {0}")]
    Embedding(String),

    #[error("LLM 调用失败: {0}")]
    Llm(String),

    #[error("参数无效: {0}")]
    InvalidArgument(String),

    #[error("记录未找到: {0}")]
    NotFound(String),

    #[error("序列化/反序列化失败: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("外部服务错误: {0}")]
    External(String),
}

impl ServiceError {
    pub fn embedding(msg: impl Into<String>) -> Self {
        ServiceError::Embedding(msg.into())
    }

    pub fn llm(msg: impl Into<String>) -> Self {
        ServiceError::Llm(msg.into())
    }

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        ServiceError::InvalidArgument(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        ServiceError::NotFound(msg.into())
    }

    pub fn external(msg: impl Into<String>) -> Self {
        ServiceError::External(msg.into())
    }
}
