use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::storage::{entity_id, MemoryStore};
use crate::tools::memory::base::{MemoryItem, RetrieveRequest};
use crate::tools::memory::extractor::EntityExtractorAgent;
use crate::tools::memory::strategy::{MemoryStrategy, StorageScope};

/// 语义记忆策略。
///
/// - 持久化到 PG + Neo4j 实体引用图。
/// - add 时抽取实体/关系，写图。
/// - retrieve 时融合向量相似度和图相关度。
pub struct SemanticStrategy {
    extractor: Arc<Mutex<EntityExtractorAgent>>,
}

impl SemanticStrategy {
    pub fn new(llm: AgentsLLM) -> Self {
        Self {
            extractor: Arc::new(Mutex::new(EntityExtractorAgent::new(llm))),
        }
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
