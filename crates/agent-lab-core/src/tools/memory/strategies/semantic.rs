use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::storage::{entity_id, MemoryStore};
use crate::tools::memory::base::{ConflictResolution, MemoryItem, RetrieveRequest};
use crate::tools::memory::conflict_resolver::{ConflictCheckRequest, MemoryConflictResolver};
use crate::tools::memory::extractor::EntityExtractorAgent;
use crate::tools::memory::strategy::{MemoryStrategy, StorageScope};

/// 语义记忆策略。
///
/// - 持久化到 PG + Neo4j 实体引用图。
/// - add 时抽取实体/关系，写图。
/// - retrieve 时融合向量相似度和图相关度。
/// - 可选支持冲突裁决：新增前会查重、合并互补事实、失效被覆盖的旧记忆。
pub struct SemanticStrategy {
    extractor: Arc<Mutex<EntityExtractorAgent>>,
    resolver: Option<Arc<Mutex<MemoryConflictResolver>>>,
}

impl SemanticStrategy {
    /// 启用冲突裁决的语义记忆策略。
    pub fn new(llm: AgentsLLM) -> Self {
        Self {
            extractor: Arc::new(Mutex::new(EntityExtractorAgent::new(llm.clone()))),
            resolver: Some(Arc::new(Mutex::new(MemoryConflictResolver::new(llm)))),
        }
    }

    /// 不启用冲突裁决的语义记忆策略（测试或低资源场景使用）。
    pub fn new_without_conflict_resolution(llm: AgentsLLM) -> Self {
        Self {
            extractor: Arc::new(Mutex::new(EntityExtractorAgent::new(llm))),
            resolver: None,
        }
    }

    /// 启发式快速去重：如果候选与新增事实足够相似，直接判为重复。
    fn heuristic_duplicate(fact: &str, candidates: &[(MemoryItem, Option<f64>)]) -> Option<String> {
        let fact_lower = fact.to_lowercase();
        for (item, score) in candidates.iter().take(3) {
            if let Some(sim) = score {
                if *sim >= 0.92 {
                    let len_ratio = item.content.len() as f64 / fact.len().max(1) as f64;
                    if len_ratio >= 0.7 && len_ratio <= 1.43 {
                        return Some(item.id.clone());
                    }
                }
            }
            if item.content.to_lowercase().trim() == fact_lower.trim() {
                return Some(item.id.clone());
            }
        }
        None
    }
}

#[async_trait::async_trait]
impl MemoryStrategy for SemanticStrategy {
    fn memory_type(&self) -> &'static str {
        "semantic"
    }

    fn storage_scope(&self) -> StorageScope {
        StorageScope::PersistentWithGraph
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
        mut item: MemoryItem,
        store: &mut MemoryStore,
    ) -> Result<(), AgentLabError> {
        let id = item.id.clone();
        let mut extractor = self.extractor.lock().await;

        match extractor.extract(&item.content).await {
            Ok((entities, relations)) if !entities.is_empty() => {
                tracing::info!(
                    "[SemanticStrategy] extracted {} entities, {} relations for memory {}",
                    entities.len(),
                    relations.len(),
                    id
                );
                let entity_ids: Vec<String> = entities
                    .iter()
                    .map(|e| entity_id(&e.name, &e.entity_type))
                    .collect();

                if let Some(obj) = item.metadata.as_object_mut() {
                    obj.insert("entity_ids".to_string(), serde_json::json!(entity_ids));
                }

                store.add_with_reference_graph(item, entities, relations).await?;
            }
            Ok((entities, relations)) => {
                tracing::warn!(
                    "[SemanticStrategy] extractor returned {} entities, {} relations for memory {}; falling back to pg only",
                    entities.len(),
                    relations.len(),
                    id
                );
                store.add(item).await?;
            }
            Err(e) => {
                tracing::error!("[SemanticStrategy] extract failed: {}; falling back to pg only", e);
                store.add(item).await?;
            }
        }

        Ok(())
    }

    async fn retrieve_candidates(
        &self,
        request: &RetrieveRequest,
        store: &MemoryStore,
        _in_memory: &[MemoryItem],
    ) -> Vec<(MemoryItem, Option<f64>)> {
        let limit = request.limit.unwrap_or(5);
        let user_id = request.user_id.as_deref().unwrap_or("default_user");

        // 1. 向量检索
        let vector_hits = match store
            .search_similar(
                &request.query,
                "semantic",
                Some(user_id),
                request.session_id.as_deref(),
                request.importance_threshold,
                request.time_range,
                limit * 2,
            )
            .await
        {
            Ok(hits) => hits,
            Err(e) => {
                tracing::error!("[SemanticStrategy] vector search failed: {}", e);
                Vec::new()
            }
        };

        // 2. 抽取查询实体，做图检索
        let mut extractor = self.extractor.lock().await;
        let (query_entity_ids, query_entity_count) = match extractor.extract(&request.query).await {
            Ok((entities, _)) if !entities.is_empty() => {
                let ids: Vec<String> = entities
                    .iter()
                    .map(|e| entity_id(&e.name, &e.entity_type))
                    .collect();
                let count = ids.len();
                (ids, count)
            }
            _ => (Vec::new(), 0),
        };
        // 尽早释放锁，避免后续 IO 持有
        drop(extractor);

        let mut graph_candidates: HashMap<String, (MemoryItem, f64)> = HashMap::new();
        if !query_entity_ids.is_empty() {
            // 2.1 直接命中
            match store
                .get_memory_ids_by_entities(user_id, &query_entity_ids, limit * 2)
                .await
            {
                Ok(ids_with_counts) => {
                    for (memory_id, matched_count) in ids_with_counts {
                        match store.get(&memory_id).await {
                            Ok(Some(item)) => {
                                let score = (matched_count as f64 / query_entity_count as f64)
                                    .clamp(0.0, 1.0);
                                graph_candidates.insert(memory_id, (item, score));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!(
                                    "[SemanticStrategy] get memory {} failed: {}",
                                    memory_id,
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[SemanticStrategy] direct graph search failed: {}", e);
                }
            }

            // 2.2 关系扩散
            match store
                .get_related_memory_ids_by_entities(user_id, &query_entity_ids, 2, limit * 2)
                .await
            {
                Ok(ids_with_counts) => {
                    for (memory_id, related_count) in ids_with_counts {
                        if graph_candidates.contains_key(&memory_id) {
                            continue;
                        }
                        match store.get(&memory_id).await {
                            Ok(Some(item)) => {
                                let score = (related_count as f64 / query_entity_count as f64)
                                    .clamp(0.0, 1.0)
                                    * 0.5;
                                graph_candidates.insert(memory_id, (item, score));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!(
                                    "[SemanticStrategy] get related memory {} failed: {}",
                                    memory_id,
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[SemanticStrategy] relation graph search failed: {}", e);
                }
            }
        }

        // 3. 融合向量与图候选
        struct Scored {
            item: MemoryItem,
            vector_score: f64,
            graph_score: f64,
        }

        let mut combined: HashMap<String, Scored> = HashMap::new();
        for (distance, item) in vector_hits {
            let vector_score = (1.0 - distance).clamp(0.0, 1.0);
            combined
                .entry(item.id.clone())
                .and_modify(|s| s.vector_score = vector_score)
                .or_insert(Scored {
                    item,
                    vector_score,
                    graph_score: 0.0,
                });
        }

        for (item, graph_score) in graph_candidates.into_values() {
            combined
                .entry(item.id.clone())
                .and_modify(|s| s.graph_score = s.graph_score.max(graph_score))
                .or_insert(Scored {
                    item,
                    vector_score: 0.0,
                    graph_score,
                });
        }

        combined
            .into_values()
            .map(|s| {
                // raw_score 这里用基础相关度（未乘 importance），
                // 最终 score 在 score() 中再做 importance 加权。
                let base_relevance = s.vector_score * 0.7 + s.graph_score * 0.3;
                if let Some(obj) = s.item.metadata.as_object() {
                    // metadata 是 Value，这里只读，后续 engine 会写入 relevance_score
                    let _ = obj;
                }
                (s.item, Some(base_relevance))
            })
            .collect()
    }

    fn score(&self, item: &MemoryItem, raw_score: Option<f64>, _now_ts: i64) -> f64 {
        let base_relevance = raw_score.unwrap_or(0.0).clamp(0.0, 1.0);
        let importance_weight = 0.8 + item.importance * 0.4;
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
            _ => false,
        }
    }
}
