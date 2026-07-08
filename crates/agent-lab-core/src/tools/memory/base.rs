use chrono::Local;

#[derive(Clone, sqlx::FromRow)]
pub struct MemoryItem {
    pub id: String,
    pub user_id: String,
    pub memory_type: String,
    pub content: String,
    pub timestamp: i64,
    pub importance: f64,
    pub session_id: Option<String>,
    pub metadata: serde_json::Value,
}

impl MemoryItem {
    pub fn new(
        user_id: String,
        memory_type: String,
        content: String,
        importance: f64,
        metadata: serde_json::Value,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self {
            id,
            user_id,
            memory_type,
            content,
            session_id: Some("default_session".into()), // todo: 目前先设置成默认session，等后续多session场景在拓展。
            timestamp: Local::now().timestamp(),
            importance,
            metadata,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct RetrieveRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub time_range: Option<(i64, i64)>,
    pub importance_threshold: Option<f64>,
}

#[async_trait::async_trait]
pub trait Memory: Send + Sync {
    async fn add(&mut self, memory_item: MemoryItem) -> String;
    async fn retrieve(&mut self, request: RetrieveRequest) -> Vec<MemoryItem>;

    /// 遗忘策略入口。默认不实现，返回 0。
    async fn forget(
        &self,
        _strategy: &str,
        _threshold: f64,
        _max_age_days: i64,
    ) -> Result<usize, String> {
        Ok(0)
    }
}

#[derive(Clone)]
pub struct MemoryConfig {
    pub working_memory_capacoty: Option<usize>,
    pub max_age_minutes: Option<i64>,
    pub time_factor: f64,
}

impl MemoryConfig {
    pub fn new() -> Self {
        Self {
            working_memory_capacoty: None,
            max_age_minutes: None,
            time_factor: 0.1,
        }
    }
}

