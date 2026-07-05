use std::env;
use std::sync::Arc;
use chrono::Local;
use qdrant_client::qdrant::qdrant_client::QdrantClient;
use sqlx::PgPool;
use serde_json::{json, Value};
use pgvector::Vector;
use crate::tools::memory::embedder::{self, Embedder, OllamaEmbedder};

#[derive(Clone)]
pub struct MemoryItem {
    pub id: String,
    pub user_id: String,
    pub memory_type: String,
    pub content: String,
    pub timestamp: i64,
    pub importance: f64,
    pub session_id: String,
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
            session_id: "default_session".into(), // todo: 目前先设置成默认session，等后续多session场景在拓展。
            timestamp: Local::now().timestamp(),
            importance,
            metadata,
        }
    }
}


#[async_trait::async_trait]
pub trait Memory: Send + Sync {
    async fn add(&mut self, memory_item: MemoryItem) -> String;
    async fn retrieve(&mut self, query: &String, limit: Option<usize>, kwargs: Option<Value>) -> Vec<MemoryItem>;
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

pub struct MemoryRetriever {
    store: MemoryStore,
    config: MemoryConfig,
}

impl MemoryRetriever {
    pub fn new(store: MemoryStore, config: MemoryConfig) -> Self {
        Self {
            store,
            config
        }
    }
}

pub async fn get_db_client() -> PgPool {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("database_url is not empty");
    PgPool::connect(&database_url).await.expect("database connection build failed")
}

#[derive(Clone)]
pub struct MemoryStore {
    config: MemoryConfig,
    db: PgPool,
    embedder: Arc<dyn Embedder + Send + Sync>,
}
impl MemoryStore {
    pub fn new(config: MemoryConfig, db: PgPool, embedder: Arc<dyn Embedder + Send + Sync>) -> Self {
        Self {
            config,
            db,
            embedder,
        }
    }

    pub async fn add(&mut self, memory_item: MemoryItem) -> Result<(), String> {
        // 1. 计算 embedding                                                                                                                                                                                                      
        let embedding = self.embedder                                                                                                                                                                                             
            .encode(&memory_item.content)                                                                                                                                                                                         
            .await                                                                                                                                                                                                                
            .expect("[MemoryStore] embedding calc failed");        
        
        let pg_vector = Vector::from(embedding);  

        sqlx::query(r#"
            INSERT INTO memories (                                                                                                                                                                                               
                memory_id, user_id, memory_type, content, embedding,                                                                                                                                                              
                importance, timestamp, session_id, properties                                                                                                                                                                     
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#)
            .bind(&memory_item.id)                                                                                                                                                                                                    
            .bind(&memory_item.user_id)                                                                                                                                                                                               
            .bind(&memory_item.memory_type)                                                                                                                                                                                           
            .bind(&memory_item.content)                                                                                                                                                                                               
            .bind(pg_vector)                                                                                                                                                                                                          
            .bind(memory_item.importance as f32)                                                                                                                                                                                      
            .bind(memory_item.timestamp)                                                                                                                                                                                              
            .bind(memory_item.session_id)                                                                                                                                                                                  
            .bind(&memory_item.metadata)                                                                                                                                                                                              
            .execute(&self.db)                                                                                                                                                                                                        
            .await       
            .expect("[MemoryStore] insert failed");                                                                                                                                                                                                     
                                                                                                                                                                                                                                   
        Ok(())
    }
}
