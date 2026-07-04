use super::base::{MemoryConfig, MemoryStore, MemoryItem, Memory};
use serde_json::Value;

pub struct SemanticMemory {
    store: MemoryStore,
    config: MemoryConfig,
    memories: Vec<MemoryItem>,
}

impl SemanticMemory {
    pub fn new(config: MemoryConfig, store: MemoryStore) -> Self {
        Self {
            config,
            store,
            memories: vec![],
        }
    }
}

impl Memory for SemanticMemory {
    fn add(&mut self, memory_item: MemoryItem) -> String {
        let id = memory_item.id.clone();
        self.memories.push(memory_item);
        id
    }

    fn retrieve(&mut self, query: &String, limit: Option<usize>, _kwargs: Option<Value>) -> Vec<MemoryItem> {
        let limit = limit.unwrap_or(5);
        let query_lower = query.to_lowercase();
        let query_words: std::collections::HashSet<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(f64, &MemoryItem)> = self.memories.iter()
            .map(|m| {
                let content_lower = m.content.to_lowercase();
                let score = if content_lower.contains(&query_lower) {
                    1.0
                } else {
                    let content_words: std::collections::HashSet<&str> = content_lower.split_whitespace().collect();
                    let intersection = query_words.intersection(&content_words).count();
                    if intersection > 0 {
                        intersection as f64 / query_words.union(&content_words).count() as f64
                    } else {
                        0.0
                    }
                };
                (score, m)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().take(limit).map(|(_, m)| m.clone()).collect()
    }
}
