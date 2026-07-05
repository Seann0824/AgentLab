use std::env;
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

pub async fn get_db_client() -> sqlx::PgPool {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("database_url is not empty");
    sqlx::PgPool::connect(&database_url).await.expect("database connection build failed")
}
