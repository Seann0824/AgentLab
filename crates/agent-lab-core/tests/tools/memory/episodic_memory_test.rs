use std::sync::Arc;

use agent_lab_core::db::get_db_client;
use agent_lab_core::tools::memory::base::{MemoryConfig, MemoryItem, RetrieveRequest};
use agent_lab_core::storage::{embedder::Embedder, MemoryStore, Neo4jStore, PgStore};
use agent_lab_core::tools::memory::engine::MemoryEngine;
use agent_lab_core::tools::memory::strategies::EpisodicStrategy;

struct MockEmbedder;

#[async_trait::async_trait]
impl Embedder for MockEmbedder {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, String> {
        Ok(vec![0.1f32; 768])
    }
}

async fn create_test_store() -> MemoryStore {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL not set");
    let db = get_db_client(&database_url).await;
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

/// 该测试需要本地 PG 和 Neo4j 服务，默认不自动运行。
/// 运行前请确保 `.env` 中 DATABASE_URL / NEO4J_URL 等配置正确。
#[tokio::test]
async fn test_episodic_memory_add_and_retrieve() {
    let store = create_test_store().await;

    let mut engine = MemoryEngine::new(
        store.clone(),
        MemoryConfig::new(),
        vec![Box::new(EpisodicStrategy::new_without_conflict_resolution())],
    );

    let user_id = "test_user_episodic_add_retrieve";
    let item1 = MemoryItem::new(
        user_id.to_string(),
        "episodic".to_string(),
        "上周去了杭州西湖，天气很好".to_string(),
        0.8,
        serde_json::json!({"session_id": "session_1"}),
    );
    let item2 = MemoryItem::new(
        user_id.to_string(),
        "episodic".to_string(),
        "昨天和同事讨论了 Q4 产品规划".to_string(),
        0.7,
        serde_json::json!({"session_id": "session_1"}),
    );
    let item3 = MemoryItem::new(
        user_id.to_string(),
        "episodic".to_string(),
        " unrelated semantic fact".to_string(),
        0.5,
        serde_json::json!({"session_id": "session_2"}),
    );

    let id1 = engine.add(item1).await;
    let id2 = engine.add(item2).await;
    let id3 = engine.add(item3).await;

    let request = RetrieveRequest {
        query: "西湖".to_string(),
        limit: Some(5),
        user_id: Some(user_id.to_string()),
        ..Default::default()
    };
    let results = engine.retrieve_by_type("episodic", request).await;

    assert!(!results.is_empty(), "应该能检索到西湖相关记忆");
    assert!(
        results.iter().any(|item| item.id == id1),
        "检索结果应包含杭州西湖那条记忆"
    );

    let request2 = RetrieveRequest {
        query: "Q4 产品规划".to_string(),
        limit: Some(5),
        user_id: Some(user_id.to_string()),
        ..Default::default()
    };
    let results2 = engine.retrieve_by_type("episodic", request2).await;
    assert!(
        results2.iter().any(|item| item.id == id2),
        "检索结果应包含 Q4 产品规划那条记忆"
    );

    // 只清理本测试创建的数据，避免并行测试互相影响。
    for id in [&id1, &id2, &id3] {
        let _ = store.delete(id).await;
    }
}

#[tokio::test]
async fn test_retrieve_filters_invalidated_memories() {
    let store = create_test_store().await;

    let mut engine = MemoryEngine::new(
        store.clone(),
        MemoryConfig::new(),
        vec![Box::new(EpisodicStrategy::new_without_conflict_resolution())],
    );

    let user_id = "test_user_invalidated_filter";
    let item = MemoryItem::new(
        user_id.to_string(),
        "episodic".to_string(),
        "我的英文名是 Sean".to_string(),
        0.8,
        serde_json::json!({"session_id": "session_1"}),
    );
    let id = engine.add(item.clone()).await;

    // 手动标记为失效（模拟冲突裁决后的结果）。
    let mut invalidated = item.clone();
    invalidated.id = id.clone();
    invalidated.user_id = user_id.to_string();
    invalidated.mark_invalidated("new_id", "name changed");
    store
        .update(&id, None, None, Some(&invalidated.metadata))
        .await
        .expect("update metadata failed");

    let request = RetrieveRequest {
        query: "英文名".to_string(),
        limit: Some(5),
        user_id: Some(user_id.to_string()),
        ..Default::default()
    };
    let results = engine.retrieve_by_type("episodic", request).await;

    assert!(
        !results.iter().any(|item| item.id == id),
        "已失效的记忆不应出现在检索结果中"
    );

    // 只清理本测试创建的数据。
    let _ = store.delete(&id).await;
}
