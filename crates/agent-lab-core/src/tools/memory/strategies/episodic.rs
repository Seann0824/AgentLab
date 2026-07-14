use std::sync::Arc;

use tokio::sync::Mutex;

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::storage::MemoryStore;
use crate::tools::memory::base::{ConflictResolution, MemoryItem, RetrieveRequest};
use crate::tools::memory::conflict_resolver::{ConflictCheckRequest, MemoryConflictResolver};
use crate::tools::memory::strategy::{MemoryStrategy, StorageScope};

/// 情景记忆策略。
///
/// - 持久化到 PG/pgvector。
/// - 检索时优先向量搜索，无结果时回退关键词匹配。
/// - 评分综合考虑向量相似度、时间衰减与重要性。
/// - 可选支持冲突裁决：新增前会查重、合并互补事实、失效被覆盖的旧记忆。
pub struct EpisodicStrategy {
    resolver: Option<Arc<Mutex<MemoryConflictResolver>>>,
}

impl EpisodicStrategy {
    /// 启用冲突裁决的情景记忆策略。
    pub fn new(llm: AgentsLLM) -> Self {
        Self {
            resolver: Some(Arc::new(Mutex::new(MemoryConflictResolver::new(llm)))),
        }
    }

    /// 不启用冲突裁决的情景记忆策略（测试或低资源场景使用）。
    pub fn new_without_conflict_resolution() -> Self {
        Self { resolver: None }
    }

    /// 启发式快速去重：如果候选与新增事实足够相似，直接判为重复。
    fn heuristic_duplicate(fact: &str, candidates: &[(MemoryItem, Option<f64>)]) -> Option<String> {
        let fact_lower = fact.to_lowercase();
        for (item, score) in candidates.iter().take(3) {
            // 向量相似度极高，且内容长度接近，认为是重复。
            if let Some(sim) = score {
                if *sim >= 0.92 {
                    let len_ratio = item.content.len() as f64 / fact.len().max(1) as f64;
                    if len_ratio >= 0.7 && len_ratio <= 1.43 {
                        return Some(item.id.clone());
                    }
                }
            }
            // 内容几乎相同也视为重复。
            if item.content.to_lowercase().trim() == fact_lower.trim() {
                return Some(item.id.clone());
            }
        }
        None
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

    fn supports_conflict_resolution(&self) -> bool {
        self.resolver.is_some()
    }

    async fn resolve_conflicts(
        &self,
        new_item: &MemoryItem,
        store: &MemoryStore,
    ) -> Result<ConflictResolution, AgentLabError> {
        let results = self.resolve_conflicts_batch(std::slice::from_ref(new_item), store).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| AgentLabError::Service(crate::services::ServiceError::invalid_argument(
                "resolve_conflicts_batch returned empty results",
            )))
    }

    async fn resolve_conflicts_batch(
        &self,
        items: &[MemoryItem],
        store: &MemoryStore,
    ) -> Result<Vec<ConflictResolution>, AgentLabError> {
        let resolver = match self.resolver.as_ref() {
            Some(r) => r,
            None => return Ok(vec![ConflictResolution::add_new(); items.len()]),
        };

        // 1. 为每个事实召回候选并做启发式去重。
        let mut fast_results: std::collections::HashMap<usize, ConflictResolution> =
            std::collections::HashMap::new();
        let mut llm_requests: Vec<ConflictCheckRequest> = Vec::new();

        for (idx, item) in items.iter().enumerate() {
            let request = RetrieveRequest {
                query: item.content.clone(),
                limit: Some(5),
                user_id: Some(item.user_id.clone()),
                session_id: item.session_id.clone(),
                importance_threshold: Some(0.0),
                ..Default::default()
            };

            let candidates = self.retrieve_candidates(&request, store, &[]).await;
            let candidates: Vec<(MemoryItem, Option<f64>)> = candidates
                .into_iter()
                .filter(|(c, _)| c.is_active())
                .collect();

            if let Some(duplicate_id) = Self::heuristic_duplicate(&item.content, &candidates) {
                fast_results.insert(idx, ConflictResolution::duplicate(duplicate_id));
            } else {
                llm_requests.push(ConflictCheckRequest {
                    fact_index: idx,
                    fact_content: item.content.clone(),
                    candidates: candidates.into_iter().map(|(item, _)| item).collect(),
                });
            }
        }

        // 2. 对剩余事实批量调用 LLM 裁决。
        if !llm_requests.is_empty() {
            let index_map: Vec<usize> = llm_requests.iter().map(|r| r.fact_index).collect();
            let mut resolver = resolver.lock().await;
            let llm_results = resolver
                .resolve_batch(llm_requests)
                .await
                .map_err(|e| AgentLabError::Service(crate::services::ServiceError::llm(e)))?;
            for (idx, resolution) in llm_results.into_iter().enumerate() {
                let original_idx = index_map[idx];
                fast_results.insert(original_idx, resolution);
            }
        }

        // 3. 按输入顺序还原结果。
        Ok((0..items.len())
            .map(|idx| {
                fast_results
                    .get(&idx)
                    .cloned()
                    .unwrap_or_else(ConflictResolution::add_new)
            })
            .collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(content: &str) -> MemoryItem {
        MemoryItem::new(
            "u1".into(),
            "episodic".into(),
            content.into(),
            0.5,
            serde_json::json!({}),
        )
    }

    #[test]
    fn test_heuristic_duplicate_exact_match() {
        let fact = "My English name is Sean";
        let item = make_item("My English name is Sean");
        let candidates = vec![(item, None)];
        assert!(EpisodicStrategy::heuristic_duplicate(fact, &candidates).is_some());
    }

    #[test]
    fn test_heuristic_duplicate_high_similarity() {
        let fact = "My English name is Sean";
        let item = make_item("My English name is Sean");
        let candidates = vec![(item, Some(0.95))];
        assert!(EpisodicStrategy::heuristic_duplicate(fact, &candidates).is_some());
    }

    #[test]
    fn test_heuristic_duplicate_no_match() {
        let fact = "My English name is Sean";
        let item = make_item("I have a cat named Mimi");
        let candidates = vec![(item, Some(0.5))];
        assert!(EpisodicStrategy::heuristic_duplicate(fact, &candidates).is_none());
    }
}
