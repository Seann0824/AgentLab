use std::sync::Arc;

use agent_lab_core::db::get_db_client;
use agent_lab_core::tools::memory::base::{MemoryConfig, MemoryItem, RetrieveRequest};
use agent_lab_core::tools::memory::episodic_memory::EpisodicMemory;
use agent_lab_core::storage::{MemoryStore, Neo4jStore, PgStore, embedder::Embedder};
use agent_lab_core::tools::memory::Memory;

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

async fn cleanup_episodes(store: &MemoryStore) {
    let _ = store.clear_by_type("episodic").await;
}

/// 该测试需要本地 PG 和 Neo4j 服务，默认不自动运行。
/// 运行前请确保 `.env` 中 DATABASE_URL / NEO4J_URL 等配置正确。
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
