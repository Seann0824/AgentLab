use super::base::{MemoryConfig, MmeoryStore, MemoryItem, Memory};
use serde_json::Value;

pub struct EpisodicMemory {
    store: MmeoryStore,
    config: MemoryConfig,
}
impl EpisodicMemory {
    pub fn new(config: MemoryConfig, store: MmeoryStore) -> Self {
        Self {
            config,
            store
        }
    }
}

impl Memory for EpisodicMemory {
    fn add(&mut self, memory_item: MemoryItem) -> String {
        todo!()
    }

    fn retrieve(&mut self, query: &String, limit: Option<usize>, kwargs: Option<Value>) -> Vec<MemoryItem> {
        todo!()
    }
}
