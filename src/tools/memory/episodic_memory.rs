use chrono::Local;
use serde_json::Value;

use super::base::{Memory, MemoryConfig, MemoryItem, RetrieveRequest};
use super::storage::MemoryStore;

pub struct EpisodicMemory {
    store: MemoryStore,
    config: MemoryConfig,
}

impl EpisodicMemory {
    pub fn new(config: MemoryConfig, store: MemoryStore) -> Self {
        Self { config, store }
    }

    pub async fn update(
        &self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f64>,
        metadata: Option<&Value>,
    ) -> Result<bool, String> {
        self.store
            .update(memory_id, content, importance, metadata)
            .await
    }

    pub async fn remove(&self, memory_id: &str) -> Result<bool, String> {
        self.store.delete(memory_id).await
    }

    pub async fn clear(&self) -> Result<u64, String> {
        self.store.clear_by_type("episodic").await
    }

    pub async fn forget(
        &self,
        strategy: &str,
        threshold: f64,
        max_age_days: i64,
    ) -> Result<usize, String> {
        let now_ts = Local::now().timestamp();
        let cutoff_ts = now_ts - max_age_days * 86400;

        let episodes = self.store.list_by_type("episodic", None, None).await?;
        let mut to_remove = Vec::new();

        for episode in &episodes {
            let should_forget = match strategy {
                "importance_based" => episode.importance < threshold,
                "time_based" => episode.timestamp < cutoff_ts,
                "capacity_based" => {
                    let max_capacity = self.config.working_memory_capacoty.unwrap_or(1000);
                    if episodes.len() > max_capacity {
                        let mut sorted: Vec<&MemoryItem> = episodes.iter().collect();
                        sorted.sort_by(|a, b| a.importance.total_cmp(&b.importance));
                        let excess = episodes.len() - max_capacity;
                        sorted.iter().take(excess).any(|e| e.id == episode.id)
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if should_forget {
                to_remove.push(episode.id.clone());
            }
        }

        let mut forgotten = 0usize;
        for id in to_remove {
            if self.store.delete(&id).await? {
                forgotten += 1;
            }
        }

        Ok(forgotten)
    }

    pub async fn get_all(&self) -> Result<Vec<MemoryItem>, String> {
        self.store.list_by_type("episodic", None, None).await
    }

    pub async fn get_stats(&self, user_id: Option<&str>) -> Result<Value, String> {
        let count = self.store.count_by_type("episodic", user_id).await?;
        let avg_importance = self
            .store
            .avg_importance_by_type("episodic", user_id)
            .await?
            .unwrap_or(0.0);
        let time_span_days = self
            .store
            .time_span_days_by_type("episodic", user_id)
            .await?
            .unwrap_or(0.0);

        Ok(serde_json::json!({
            "count": count,
            "avg_importance": avg_importance,
            "time_span_days": time_span_days,
            "memory_type": "episodic"
        }))
    }
}

#[async_trait::async_trait]
impl Memory for EpisodicMemory {
    async fn add(&mut self, memory_item: MemoryItem) -> String {
        let id = memory_item.id.clone();
        if let Err(e) = self.store.add(memory_item).await {
            tracing::error!("[EpisodicMemory] store.add failed: {}", e);
        }
        id
    }

    async fn retrieve(&mut self, request: RetrieveRequest) -> Vec<MemoryItem> {
        let user_id = request.user_id.as_deref();
        let session_id = request.session_id.as_deref();
        let time_range = request.time_range;
        let importance_threshold = request.importance_threshold;
        let limit = request.limit.unwrap_or(5);
        let vector_limit = (limit * 5).max(20);

        let now_ts = Local::now().timestamp();
        let mut results: Vec<(f64, MemoryItem)> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 1. 向量检索（PG + pgvector）
        match self
            .store
            .search_similar(
                &request.query,
                "episodic",
                user_id,
                session_id,
                importance_threshold,
                time_range,
                vector_limit,
            )
            .await
        {
            Ok(hits) => {
                for (distance, mut memory_item) in hits {
                    if seen.contains(&memory_item.id) {
                        continue;
                    }

                    // 跳过已遗忘的记忆
                    if memory_item
                        .metadata
                        .get("forgotten")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        continue;
                    }

                    // pgvector <=> 返回 cosine distance，转换为 similarity
                    let vec_score = 1.0 - distance;
                    let age_days = ((now_ts - memory_item.timestamp) as f64 / 86400.0).max(0.0);
                    let recency_score = 1.0 / (1.0 + age_days);
                    let importance_weight = 0.8 + memory_item.importance * 0.4;
                    let base_relevance = vec_score * 0.8 + recency_score * 0.2;
                    let combined = base_relevance * importance_weight;

                    memory_item.metadata["relevance_score"] = serde_json::json!(combined);
                    memory_item.metadata["vector_score"] = serde_json::json!(vec_score);
                    memory_item.metadata["recency_score"] = serde_json::json!(recency_score);

                    results.push((combined, memory_item.clone()));
                    seen.insert(memory_item.id);
                }
            }
            Err(e) => {
                tracing::error!("[EpisodicMemory] vector search failed: {}", e);
            }
        }

        // 2. 若向量检索无结果，回退到关键词匹配（直接查 PG）
        if results.is_empty() {
            match self
                .store
                .keyword_search(
                    &request.query,
                    "episodic",
                    user_id,
                    session_id,
                    importance_threshold,
                    time_range,
                )
                .await
            {
                Ok(hits) => {
                    for mut memory_item in hits {
                        if seen.contains(&memory_item.id) {
                            continue;
                        }

                        if memory_item
                            .metadata
                            .get("forgotten")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            continue;
                        }

                        let age_days = ((now_ts - memory_item.timestamp) as f64 / 86400.0).max(0.0);
                        let recency_score = 1.0 / (1.0 + age_days);
                        let importance_weight = 0.8 + memory_item.importance * 0.4;
                        let keyword_score = 0.5;
                        let base_relevance = keyword_score * 0.8 + recency_score * 0.2;
                        let combined = base_relevance * importance_weight;

                        memory_item.metadata["relevance_score"] = serde_json::json!(combined);
                        memory_item.metadata["recency_score"] = serde_json::json!(recency_score);

                        results.push((combined, memory_item.clone()));
                        seen.insert(memory_item.id);
                    }
                }
                Err(e) => {
                    tracing::error!("[EpisodicMemory] keyword fallback failed: {}", e);
                }
            }
        }

        results.sort_by(|a, b| b.0.total_cmp(&a.0));
        results
            .into_iter()
            .take(limit)
            .map(|(_, item)| item)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::tools::memory::base::{MemoryConfig, get_db_client};
    use crate::tools::memory::storage::{MemoryStore, Neo4jStore, PgStore, embedder::Embedder};

    struct MockEmbedder;

    #[async_trait::async_trait]
    impl Embedder for MockEmbedder {
        async fn encode(&self, _text: &str) -> Result<Vec<f32>, String> {
            Ok(vec![0.1f32; 768])
        }
    }

    async fn create_test_store() -> MemoryStore {
        dotenvy::dotenv().ok();
        let db = get_db_client().await;
        let config = MemoryConfig::new();
        let pg_store = PgStore::new(config.clone(), db);
        let neo4j_store = Neo4jStore::new(
            std::env::var("NEO4J_URL").unwrap_or_else(|_| "neo4j://127.0.0.1:7687".into()),
            std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into()),
            std::env::var("NEO4J_PASSWORD").unwrap_or_default(),
        )
        .await
        .expect("neo4j test connection failed");
        let embedder: Arc<dyn Embedder + Send + Sync> = Arc::new(MockEmbedder);
        MemoryStore::new(config, pg_store, neo4j_store, embedder)
    }

    async fn cleanup_episodes(store: &MemoryStore) {
        let _ = store.clear_by_type("episodic").await;
    }

    #[tokio::test]
    async fn test_episodic_memory_add_and_retrieve() {
        let store = create_test_store().await;
        cleanup_episodes(&store).await;

        let mut episodic = EpisodicMemory::new(MemoryConfig::new(), store.clone());

        let item1 = MemoryItem::new(
            "test_user".to_string(),
            "episodic".to_string(),
            "上周去了杭州西湖，天气很好".to_string(),
            0.8,
            serde_json::json!({"session_id": "session_1"}),
        );
        let item2 = MemoryItem::new(
            "test_user".to_string(),
            "episodic".to_string(),
            "昨天和同事讨论了 Q4 产品规划".to_string(),
            0.7,
            serde_json::json!({"session_id": "session_1"}),
        );
        let item3 = MemoryItem::new(
            "test_user".to_string(),
            "episodic".to_string(),
            " unrelated semantic fact".to_string(),
            0.5,
            serde_json::json!({"session_id": "session_2"}),
        );

        let id1 = episodic.add(item1).await;
        let id2 = episodic.add(item2).await;
        let _id3 = episodic.add(item3).await;

        let request = RetrieveRequest {
            query: "西湖".to_string(),
            limit: Some(5),
            user_id: Some("test_user".to_string()),
            ..Default::default()
        };
        let results = episodic.retrieve(request).await;

        assert!(!results.is_empty(), "应该能检索到西湖相关记忆");
        assert!(
            results.iter().any(|item| item.id == id1),
            "检索结果应包含杭州西湖那条记忆"
        );

        let request2 = RetrieveRequest {
            query: "Q4 产品规划".to_string(),
            limit: Some(5),
            user_id: Some("test_user".to_string()),
            ..Default::default()
        };
        let results2 = episodic.retrieve(request2).await;
        assert!(
            results2.iter().any(|item| item.id == id2),
            "检索结果应包含 Q4 产品规划那条记忆"
        );

        cleanup_episodes(&store).await;
    }
}
