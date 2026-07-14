use std::collections::HashSet;

use crate::error::AgentLabError;
use crate::storage::MemoryStore;
use crate::tools::memory::base::{MemoryItem, RetrieveRequest};
use crate::tools::memory::strategy::{MemoryStrategy, StorageScope};

/// 感知记忆策略。
///
/// - 仅进程内存储。
/// - 检索时使用简单关键词匹配。
pub struct PerceptualStrategy;

impl PerceptualStrategy {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl MemoryStrategy for PerceptualStrategy {
    fn memory_type(&self) -> &'static str {
        "perceptual"
    }

    fn storage_scope(&self) -> StorageScope {
        StorageScope::InMemory
    }

    async fn enrich_and_store(
        &self,
        _item: MemoryItem,
        _store: &mut MemoryStore,
    ) -> Result<(), AgentLabError> {
        // 感知记忆不持久化，由 MemoryEngine 维护在内存中。
        Ok(())
    }

    async fn retrieve_candidates(
        &self,
        request: &RetrieveRequest,
        _store: &MemoryStore,
        in_memory: &[MemoryItem],
    ) -> Vec<(MemoryItem, Option<f64>)> {
        let query_lower = request.query.to_lowercase();
        let query_words: HashSet<&str> = query_lower.split_whitespace().collect();

        in_memory
            .iter()
            .filter(|item| !is_forgotten(item))
            .map(|item| {
                let content_lower = item.content.to_lowercase();
                let score = if content_lower.contains(&query_lower) {
                    1.0
                } else {
                    let content_words: HashSet<&str> = content_lower.split_whitespace().collect();
                    let intersection = query_words.intersection(&content_words).count();
                    if intersection > 0 {
                        intersection as f64 / query_words.union(&content_words).count() as f64
                    } else {
                        0.0
                    }
                };
                (item.clone(), Some(score))
            })
            .filter(|(_, score)| score.unwrap_or(0.0) > 0.0)
            .collect()
    }

    fn score(&self, item: &MemoryItem, raw_score: Option<f64>, now_ts: i64) -> f64 {
        let keyword_score = raw_score.unwrap_or(0.0).clamp(0.0, 1.0);
        let age_days = ((now_ts - item.timestamp) as f64 / 86400.0).max(0.0);
        let recency_score = 1.0 / (1.0 + age_days);
        let importance_weight = 0.8 + item.importance * 0.4;
        (keyword_score * 0.8 + recency_score * 0.2) * importance_weight
    }

    fn should_forget(
        &self,
        item: &MemoryItem,
        strategy: &str,
        threshold: f64,
        max_age_days: i64,
        now_ts: i64,
    ) -> bool {
        match strategy {
            "importance_based" => item.importance < threshold,
            "time_based" => {
                let cutoff_ts = now_ts - max_age_days * 86400;
                item.timestamp < cutoff_ts
            }
            _ => false,
        }
    }
}

fn is_forgotten(item: &MemoryItem) -> bool {
    item.metadata
        .get("forgotten")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
