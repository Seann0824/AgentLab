use std::collections::HashMap;

use serde_json::json;

use super::base::{Memory, MemoryConfig, MemoryItem, RetrieveRequest};
use super::extractor::EntityExtractorAgent;
use super::storage::{MemoryStore, entity_id};

/// 语义记忆实现。
///
/// 只依赖 `MemoryStore`（PG + Neo4j），内部不再维护内存缓存。
/// - `add`：内部用子 agent 抽取实体/关系，把实体 id 写入 metadata，再写入 PG + Neo4j。
/// - `retrieve`：向量检索（PG/pgvector）+ 图检索（Neo4j 实体引用）混合排序，
///   图分数通过候选记忆的 metadata 中保存的 entity_ids 与查询实体重叠度计算。
pub struct SemanticMemory {
    #[allow(dead_code)]
    config: MemoryConfig,
    store: MemoryStore,
    extractor: EntityExtractorAgent,
}

impl SemanticMemory {
    pub fn new(config: MemoryConfig, store: MemoryStore, extractor: EntityExtractorAgent) -> Self {
        Self {
            config,
            store,
            extractor,
        }
    }
}

#[async_trait::async_trait]
impl Memory for SemanticMemory {
    async fn add(&mut self, mut memory_item: MemoryItem) -> String {
        let id = memory_item.id.clone();

        match self.extractor.extract(&memory_item.content).await {
            Ok((entities, relations)) if !entities.is_empty() => {
                tracing::info!(
                    "[SemanticMemory] extracted {} entities, {} relations for memory {}",
                    entities.len(),
                    relations.len(),
                    id
                );
                let entity_ids: Vec<String> = entities
                    .iter()
                    .map(|e| entity_id(&e.name, &e.entity_type))
                    .collect();

                if let Some(obj) = memory_item.metadata.as_object_mut() {
                    obj.insert("entity_ids".to_string(), json!(entity_ids));
                }

                if let Err(e) = self
                    .store
                    .add_with_reference_graph(memory_item, entities, relations)
                    .await
                {
                    tracing::error!("[SemanticMemory] add_with_reference_graph failed: {}", e);
                }
            }
            Ok((entities, relations)) => {
                tracing::warn!(
                    "[SemanticMemory] extractor returned {} entities, {} relations for memory {}; falling back to pg only",
                    entities.len(),
                    relations.len(),
                    id
                );
                if let Err(e) = self.store.add(memory_item).await {
                    tracing::error!("[SemanticMemory] store.add failed: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("[SemanticMemory] extract failed: {}; falling back to pg only", e);
                if let Err(e) = self.store.add(memory_item).await {
                    tracing::error!("[SemanticMemory] store.add failed: {}", e);
                }
            }
        }

        id
    }

    async fn retrieve(&mut self, request: RetrieveRequest) -> Vec<MemoryItem> {
        let limit = request.limit.unwrap_or(5);
        let user_id = request.user_id.as_deref().unwrap_or("default_user");

        // 1. 向量检索：PG/pgvector 返回的是 cosine distance，转成 similarity。
        let vector_hits = match self
            .store
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
                tracing::error!("[SemanticMemory] vector search failed: {}", e);
                Vec::new()
            }
        };

        // 2. 从查询中抽取实体，作为图检索的入口。
        let (query_entity_ids, query_entity_count) =
            match self.extractor.extract(&request.query).await {
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

        // 3. 图检索候选：直接命中查询实体的记忆 + 通过实体关系图扩散到的记忆。
        let mut graph_candidates: HashMap<String, (MemoryItem, f64)> = HashMap::new();

        if !query_entity_ids.is_empty() {
            // 3.1 直接命中：记忆包含查询中的实体。
            match self
                .store
                .get_memory_ids_by_entities(user_id, &query_entity_ids, limit * 2)
                .await
            {
                Ok(ids_with_counts) => {
                    for (memory_id, matched_count) in ids_with_counts {
                        match self.store.get(&memory_id).await {
                            Ok(Some(item)) => {
                                let score = (matched_count as f64 / query_entity_count as f64)
                                    .clamp(0.0, 1.0);
                                graph_candidates.insert(memory_id, (item, score));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!(
                                    "[SemanticMemory] get memory {} failed: {}",
                                    memory_id,
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[SemanticMemory] direct graph search failed: {}", e);
                }
            }

            // 3.2 关系扩散：通过实体间 RELATED_TO 找到相关记忆。
            match self
                .store
                .get_related_memory_ids_by_entities(user_id, &query_entity_ids, 2, limit * 2)
                .await
            {
                Ok(ids_with_counts) => {
                    for (memory_id, related_count) in ids_with_counts {
                        if graph_candidates.contains_key(&memory_id) {
                            continue;
                        }
                        match self.store.get(&memory_id).await {
                            Ok(Some(item)) => {
                                let score = (related_count as f64 / query_entity_count as f64)
                                    .clamp(0.0, 1.0)
                                    * 0.5; // 关系扩散降权
                                graph_candidates.insert(memory_id, (item, score));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!(
                                    "[SemanticMemory] get related memory {} failed: {}",
                                    memory_id,
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[SemanticMemory] relation graph search failed: {}", e);
                }
            }
        }

        // 4. 融合：按 vector_score * 0.7 + graph_score * 0.3 计算基础相关性，
        //    再用 importance 做 [0.8, 1.2] 的加权。
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

        let mut scored: Vec<(f64, Scored)> = combined
            .into_values()
            .map(|s| {
                let base_relevance = s.vector_score * 0.7 + s.graph_score * 0.3;
                let importance_weight = 0.8 + s.item.importance * 0.4;
                let combined_score = base_relevance * importance_weight;
                (combined_score, s)
            })
            .filter(|(score, _)| *score >= 0.1)
            .collect();

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(limit);

        // 5. 对最终分数做 softmax，得到概率分布。
        let probabilities = softmax(&scored.iter().map(|(score, _)| *score).collect::<Vec<_>>());

        scored
            .into_iter()
            .zip(probabilities.into_iter())
            .map(|((score, mut s), prob)| {
                if let Some(obj) = s.item.metadata.as_object_mut() {
                    obj.insert("combined_score".to_string(), json!(score));
                    obj.insert("vector_score".to_string(), json!(s.vector_score));
                    obj.insert("graph_score".to_string(), json!(s.graph_score));
                    obj.insert("probability".to_string(), json!(prob));
                }
                s.item
            })
            .collect()
    }
}

fn softmax(scores: &[f64]) -> Vec<f64> {
    if scores.is_empty() {
        return Vec::new();
    }
    let max_score = scores.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let exps: Vec<f64> = scores.iter().map(|s| (s - max_score).exp()).collect();
    let sum: f64 = exps.iter().sum();
    exps.into_iter().map(|e| e / sum).collect()
}
