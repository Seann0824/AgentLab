use crate::error::AgentLabError;
use crate::storage::MemoryStore;
use crate::tools::memory::base::{MemoryItem, RetrieveRequest};
use crate::tools::memory::strategy::{MemoryStrategy, StorageScope};

/// 情景记忆策略。
///
/// - 持久化到 PG/pgvector。
/// - 检索时优先向量搜索，无结果时回退关键词匹配。
/// - 评分综合考虑向量相似度、时间衰减与重要性。
pub struct EpisodicStrategy;

impl EpisodicStrategy {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl MemoryStrategy for EpisodicStrategy {
    fn memory_type(&self) -> &'static str {
        "episodic"
    }

    fn storage_scope(&self) -> StorageScope {
        StorageScope::Persistent
    }

    async fn enrich_and_store(
        &self,
        item: MemoryItem,
        store: &mut MemoryStore,
    ) -> Result<(), AgentLabError> {
        store.add(item).await?;
        Ok(())
    }

    async fn retrieve_candidates(
        &self,
        request: &RetrieveRequest,
        store: &MemoryStore,
        _in_memory: &[MemoryItem],
    ) -> Vec<(MemoryItem, Option<f64>)> {
        let limit = request.limit.unwrap_or(5);
        let vector_limit = (limit * 5).max(20);
        let user_id = request.user_id.as_deref();

        // 1. 向量检索
        let mut results = match store
            .search_similar(
                &request.query,
                "episodic",
                user_id,
                request.session_id.as_deref(),
                request.importance_threshold,
                request.time_range,
                vector_limit,
            )
            .await
        {
            Ok(hits) => hits
                .into_iter()
                .map(|(distance, item)| {
                    let similarity = (1.0 - distance).clamp(0.0, 1.0);
                    (item, Some(similarity))
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::error!("[EpisodicStrategy] vector search failed: {}", e);
                Vec::new()
            }
        };

        // 2. 向量为空则回退关键词
        if results.is_empty() {
            match store
                .keyword_search(
                    &request.query,
                    "episodic",
                    user_id,
                    request.session_id.as_deref(),
                    request.importance_threshold,
                    request.time_range,
                )
                .await
            {
                Ok(hits) => {
                    results = hits.into_iter().map(|item| (item, Some(0.5))).collect();
                }
                Err(e) => {
                    tracing::error!("[EpisodicStrategy] keyword fallback failed: {}", e);
                }
            }
        }

        results
    }

    fn score(&self, item: &MemoryItem, raw_score: Option<f64>, now_ts: i64) -> f64 {
        let vec_score = raw_score.unwrap_or(0.0).clamp(0.0, 1.0);
        let age_days = ((now_ts - item.timestamp) as f64 / 86400.0).max(0.0);
        let recency_score = 1.0 / (1.0 + age_days);
        let importance_weight = 0.8 + item.importance * 0.4;
        let base_relevance = vec_score * 0.8 + recency_score * 0.2;
        base_relevance * importance_weight
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
            // capacity_based 由 MemoryEngine 统一按容量裁剪
            _ => false,
        }
    }
}
