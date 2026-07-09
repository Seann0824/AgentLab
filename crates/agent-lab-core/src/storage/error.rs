/// 存储层统一错误类型。
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("数据库错误: {0}")]
    Database(String),

    #[error("embedding 失败: {0}")]
    Embedding(String),

    #[error("图数据库错误: {0}")]
    Graph(String),

    #[error("记录未找到")]
    NotFound,

    #[error("无效参数: {0}")]
    InvalidArgument(String),
}

impl StorageError {
    pub fn database(msg: impl Into<String>) -> Self {
        StorageError::Database(msg.into())
    }

    pub fn embedding(msg: impl Into<String>) -> Self {
        StorageError::Embedding(msg.into())
    }

    pub fn graph(msg: impl Into<String>) -> Self {
        StorageError::Graph(msg.into())
    }

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        StorageError::InvalidArgument(msg.into())
    }
}
