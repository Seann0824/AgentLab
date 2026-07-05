use agent_lab::tools::memory::storage::{entity_id, Entity, Neo4jStore, Relation};

/// 这个测试验证 Neo4j 在“只存引用、全文在 PG”模式下的基本可用性：
///
/// 1. 连接到 Neo4j。
/// 2. 写入两条记忆的实体引用图（模拟业务层根据 name+type 计算 entity id）。
/// 3. 通过共享实体（小明）跨记忆找到关联的 memory_id。
/// 4. 清理测试数据。
#[tokio::test]
async fn test_neo4j_reference_graph_crud() {
    dotenvy::dotenv().ok();

    let uri = std::env::var("NEO4J_URL").unwrap_or_else(|_| "neo4j://127.0.0.1:7687".into());
    let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
    let password = std::env::var("NEO4J_PASSWORD").unwrap_or_default();

    let store = Neo4jStore::new(uri, user, password)
        .await
        .expect("connect to neo4j failed");

    let user_id = "test_user_ref";
    let memory_a = "mem_ref_a";
    let memory_b = "mem_ref_b";
    let memory_type = "episodic";

    // 清理历史测试数据，避免重复运行污染结果。
    let _ = store.delete_reference_graph(memory_a, user_id).await;
    let _ = store.delete_reference_graph(memory_b, user_id).await;

    // 业务层根据 name+type 计算稳定 entity id。
    let id_xiaoming = entity_id("小明", "PERSON");
    let id_hangzhou = entity_id("杭州", "LOCATION");
    let id_xihu = entity_id("西湖", "LOCATION");

    // memory_a：小明 -[去过]-> 杭州
    let entities_a = vec![
        Entity {
            id: id_xiaoming.clone(),
            name: "小明".into(),
            entity_type: "PERSON".into(),
        },
        Entity {
            id: id_hangzhou.clone(),
            name: "杭州".into(),
            entity_type: "LOCATION".into(),
        },
    ];
    let relations_a = vec![Relation {
        from_id: id_xiaoming.clone(),
        to_id: id_hangzhou.clone(),
        relation_type: "去过".into(),
        memory_id: memory_a.into(),
        user_id: user_id.into(),
    }];
    store
        .add_reference_graph(memory_a, user_id, memory_type, &entities_a, &relations_a)
        .await
        .expect("add reference graph for memory_a failed");

    // memory_b：小明 -[游览]-> 西湖
    // 其中“小明”与 memory_a 共享同一个 entity id，从而把两条记忆在图上连接起来。
    let entities_b = vec![
        Entity {
            id: id_xiaoming.clone(),
            name: "小明".into(),
            entity_type: "PERSON".into(),
        },
        Entity {
            id: id_xihu.clone(),
            name: "西湖".into(),
            entity_type: "LOCATION".into(),
        },
    ];
    let relations_b = vec![Relation {
        from_id: id_xiaoming.clone(),
        to_id: id_xihu.clone(),
        relation_type: "游览".into(),
        memory_id: memory_b.into(),
        user_id: user_id.into(),
    }];
    store
        .add_reference_graph(memory_b, user_id, memory_type, &entities_b, &relations_b)
        .await
        .expect("add reference graph for memory_b failed");

    // 从 memory_a 出发，深度 2，应该能通过共享的“小明”跨记忆找到 memory_b。
    let related = store
        .get_related_memory_ids(memory_a, user_id, 2, 10)
        .await
        .expect("get related memory ids failed");

    assert!(
        related.contains(&memory_b.to_string()),
        "expected to find memory_b from memory_a through entity graph, got {:?}",
        related
    );

    // 清理。
    store
        .delete_reference_graph(memory_a, user_id)
        .await
        .expect("delete memory_a reference graph failed");
    store
        .delete_reference_graph(memory_b, user_id)
        .await
        .expect("delete memory_b reference graph failed");

    // 删除后应该再也查不到关联。
    let related_after_cleanup = store
        .get_related_memory_ids(memory_a, user_id, 2, 10)
        .await
        .expect("get related memory ids after cleanup failed");
    assert!(
        !related_after_cleanup.contains(&memory_b.to_string()),
        "related memory should be removed after cleanup"
    );
}
