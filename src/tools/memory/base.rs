use std::env;

use qdrant_client::qdrant::qdrant_client::QdrantClient;
use serde_json::{json, Value};
use qdrant_client::{Qdrant, Payload};
use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder, PointStruct, DocumentBuilder, UpsertPointsBuilder, QueryPointsBuilder, Query};

#[derive(Clone)]
pub struct MemoryItem {
    pub id: String,
    pub memory_type: String,
    pub content: String,
    pub timestamp: i64,
    pub importance: f64,
}


pub trait Memory: Send + Sync {
    fn add(&mut self, memory_item: MemoryItem) -> String;
    fn retrieve(&mut self, query: &String, limit: Option<usize>, kwargs: Option<Value>) -> Vec<MemoryItem>;
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


pub fn get_qdrant_client() -> Qdrant {
    dotenvy::dotenv().ok();
    let url = env::var("QDRANT_KEY").expect("API_KEY is not valid");
    let key = env::var("QDRANT_ENDPOINT").expect("BASE_URL is not valid");
    let client = Qdrant::from_url(&url)
        .api_key(key)
        .build().unwrap();
    client
}





#[derive(Clone)]
pub struct MemoryStore {}
impl MemoryStore {
    pub fn new(config: MemoryConfig) -> Self {
        Self {  }
    }
}
