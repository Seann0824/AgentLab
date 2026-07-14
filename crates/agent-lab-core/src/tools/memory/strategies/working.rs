use scirs2_text::vectorize::{TfidfVectorizer, Vectorizer};

use crate::error::AgentLabError;
use crate::storage::MemoryStore;
use crate::tools::memory::base::{MemoryConfig, MemoryItem, RetrieveRequest};
use crate::tools::memory::strategy::{MemoryStrategy, StorageScope};

/// 工作记忆策略。
///
/// - 仅进程内存储。
/// - 检索时使用 TF-IDF 计算语义相似度。
/// - 受容量和过期时间约束。
pub struct WorkingStrategy {
    config: MemoryConfig,
}

impl WorkingStrategy {
    pub fn new(config: MemoryConfig) -> Self {
        Self { config }
    }

    fn max_capacity(&self) -> usize {
        self.config.working_memory_capacoty.unwrap_or(50)
    }
}

#[async_trait::async_trait]
impl MemoryStrategy for WorkingStrategy {
    fn memory_type(&self) -> &'static str {
        "working"
    }

    fn storage_scope(&self) -> StorageScope {
        StorageScope::InMemory
    }

    async fn enrich_and_store(
        &self,
        _item: MemoryItem,
        _store: &mut MemoryStore,
    ) -> Result<(), AgentLabError> {
        // 工作记忆不持久化，由 MemoryEngine 维护在内存中。
        Ok(())
    }

    async fn retrieve_candidates(
        &self,
        request: &RetrieveRequest,
        _store: &MemoryStore,
        in_memory: &[MemoryItem],
    ) -> Vec<(MemoryItem, Option<f64>)> {
        if in_memory.is_empty() {
            return Vec::new();
        }

        let query = &request.query;
        let mut documents: Vec<&str> = in_memory
            .iter()
            .map(|memory| memory.content.as_str())
            .collect();
        documents.insert(0, query.as_str());

        let mut tfidf = TfidfVectorizer::new(false, true, Some("l2".to_string()));
        let matrix = match tfidf.fit_transform(&documents) {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };

        let query_vec = matrix.row(0);
        let mut similarities = Vec::with_capacity(in_memory.len());
        for i in 1..matrix.nrows() {
            let memory_vec = matrix.row(i);
            let similarity = query_vec.dot(&memory_vec);
            similarities.push(similarity);
        }

        in_memory
            .iter()
            .zip(similarities.into_iter())
            .filter(|(_, score)| *score > 0.0)
            .map(|(memory, score)| (memory.clone(), Some(score)))
            .collect()
    }

    fn score(&self, item: &MemoryItem, raw_score: Option<f64>, now_ts: i64) -> f64 {
        let tfidf_score = raw_score.unwrap_or(0.0).clamp(0.0, 1.0);
        let age_minutes = ((now_ts - item.timestamp) as f64 / 60.0).max(0.0);
        let recency_score = 1.0 / (1.0 + age_minutes / 10.0);
        let importance_weight = 0.8 + item.importance * 0.4;
        // 容量压力：越接近上限，低分记忆越被压制
        let capacity_pressure = 1.0; // engine 层统一处理容量，策略本身不做额外压制
        (tfidf_score * 0.7 + recency_score * 0.3) * importance_weight * capacity_pressure
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
            "capacity_based" => {
                // capacity_based 的具体裁剪由 MemoryEngine 统一按优先级排序后执行，
                // 这里只提供一个基于优先级的判断辅助。
                let age_minutes = ((now_ts - item.timestamp) as f64 / 60.0).max(0.0);
                let recency = 1.0 / (1.0 + age_minutes / 10.0);
                let priority = item.importance * 0.6 + recency * 0.4;
                priority < threshold
            }
            _ => false,
        }
    }

    fn capacity(&self) -> Option<usize> {
        Some(self.max_capacity())
    }
}
