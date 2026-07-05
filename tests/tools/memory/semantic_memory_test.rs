use agent_lab::tools::memory::Memory;
use agent_lab::tools::memory::base::{MemoryConfig, MemoryItem, RetrieveRequest};
use agent_lab::tools::memory::semantic_memory::SemanticMemory;

fn make_fact(content: &str) -> MemoryItem {
    MemoryItem::new(
        "test_user".into(),
        "semantic".into(),
        content.into(),
        0.8,
        serde_json::json!({}),
    )
}

#[tokio::test]
async fn test_semantic_memory_exact_match() {
    let mut memory = SemanticMemory::new(MemoryConfig::new());

    let fact1 = make_fact("水的化学式是 H2O");
    let fact2 = make_fact("地球是太阳系第三颗行星");
    let fact3 = make_fact("光速约为每秒 30 万公里");

    memory.add(fact1.clone()).await;
    memory.add(fact2.clone()).await;
    memory.add(fact3.clone()).await;

    let results = memory
        .retrieve(RetrieveRequest {
            query: "水的化学式".into(),
            limit: Some(3),
            ..Default::default()
        })
        .await;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, fact1.id);
}

#[tokio::test]
async fn test_semantic_memory_partial_overlap() {
    let mut memory = SemanticMemory::new(MemoryConfig::new());

    let fact1 = make_fact("Python is an interpreted programming language");
    let fact2 = make_fact("Rust is a systems programming language");
    let fact3 = make_fact("JavaScript is used for frontend development");

    memory.add(fact1.clone()).await;
    memory.add(fact2.clone()).await;
    memory.add(fact3.clone()).await;

    // "language development" 没有任何一条记忆的完整内容包含这个短语，
    // 但 Jaccard 重叠度会让三条记忆都被召回。
    let results = memory
        .retrieve(RetrieveRequest {
            query: "language development".into(),
            limit: Some(3),
            ..Default::default()
        })
        .await;

    assert_eq!(results.len(), 3);
    assert!(results.iter().any(|m| m.id == fact1.id));
    assert!(results.iter().any(|m| m.id == fact2.id));
    assert!(results.iter().any(|m| m.id == fact3.id));
}

#[tokio::test]
async fn test_semantic_memory_ranking() {
    let mut memory = SemanticMemory::new(MemoryConfig::new());

    let fact1 = make_fact("猫是哺乳动物，喜欢捉老鼠");
    let fact2 = make_fact("狗是哺乳动物，经常被人类驯化为宠物");
    let fact3 = make_fact("金鱼是一种常见的观赏鱼类");

    memory.add(fact1.clone()).await;
    memory.add(fact2.clone()).await;
    memory.add(fact3.clone()).await;

    // "哺乳动物" 应该优先返回猫和狗，而不是金鱼
    let results = memory
        .retrieve(RetrieveRequest {
            query: "哺乳动物".into(),
            limit: Some(2),
            ..Default::default()
        })
        .await;

    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|m| m.id == fact1.id));
    assert!(results.iter().any(|m| m.id == fact2.id));
    assert!(!results.iter().any(|m| m.id == fact3.id));
}

#[tokio::test]
async fn test_semantic_memory_no_match() {
    let mut memory = SemanticMemory::new(MemoryConfig::new());
    memory.add(make_fact("太阳是一颗恒星")).await;

    let results = memory
        .retrieve(RetrieveRequest {
            query: "量子力学".into(),
            limit: Some(3),
            ..Default::default()
        })
        .await;

    assert!(results.is_empty());
}
