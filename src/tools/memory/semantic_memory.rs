use super::base::{Memory, MemoryConfig, MemoryItem, RetrieveRequest};

pub struct SemanticMemory {
    #[allow(dead_code)]
    config: MemoryConfig,
    memories: Vec<MemoryItem>,
}

impl SemanticMemory {
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            config,
            memories: vec![],
        }
    }
}

#[async_trait::async_trait]
impl Memory for SemanticMemory {
    async fn add(&mut self, memory_item: MemoryItem) -> String {
        let id = memory_item.id.clone();
        self.memories.push(memory_item);
        id
    }

    async fn retrieve(&mut self, request: RetrieveRequest) -> Vec<MemoryItem> {
        let limit = request.limit.unwrap_or(5);
        let query_lower = request.query.to_lowercase();
        let query_words: std::collections::HashSet<&str> =
            query_lower.split_whitespace().collect();

        let mut scored: Vec<(f64, &MemoryItem)> = self
            .memories
            .iter()
            .map(|m| {
                let content_lower = m.content.to_lowercase();
                let score = if content_lower.contains(&query_lower) {
                    1.0
                } else {
                    let content_words: std::collections::HashSet<&str> =
                        content_lower.split_whitespace().collect();
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
        scored
            .into_iter()
            .take(limit)
            .map(|(_, m)| m.clone())
            .collect()
    }
}


