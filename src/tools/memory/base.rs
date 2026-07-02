use serde_json::Value;


#[derive(Clone)]
pub struct MemoryItem {
    pub id: String,
    pub memory_type: String,
    pub content: String,
    pub timestamp: u64,
    pub importance: f32,
}


pub trait Memory: Send + Sync {
    fn add(&mut self, memory_item: MemoryItem) -> String;
    fn retrieve(&mut self, query: &String, limit: Option<usize>, kwargs: Option<Value>) -> Vec<MemoryItem>;
}


#[derive(Clone)]
pub struct MemoryConfig {
    pub working_memory_capacoty: Option<usize>,
    pub max_age_minutes: Option<usize>,
}

impl MemoryConfig {
    pub fn new() -> Self {
        Self {
            working_memory_capacoty: None,
            max_age_minutes: None,
        }
    }
}

#[derive(Clone)]
pub struct MmeoryStore {}
impl MmeoryStore {
    pub fn new(config: MemoryConfig) -> Self {
        Self {  }
    }
}

pub struct MemoryRetriever {
    store: MmeoryStore,
    config: MemoryConfig,
}

impl MemoryRetriever {
    pub fn new(store: MmeoryStore, config: MemoryConfig) -> Self {
        Self {
            store,
            config
        }
    }
}
