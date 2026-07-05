pub mod embedder;
pub mod neo4j;
pub mod pg;

pub use embedder::OllamaEmbedder;
pub use neo4j::{Entity, Neo4jStore, Relation};
pub use pg::PgStore;

use std::collections::HashMap;
use std::sync::Arc;
use pgvector::Vector;
use serde_json::Value;

use crate::tools::memory::base::{MemoryConfig, MemoryItem};
use crate::tools::memory::storage::embedder::Embedder;
use crate::tools::memory::storage::neo4j::{EntityInput, RelationInput};

/// 组合存储：PG 负责向量/结构化检索，Neo4j 负责图关系，Embedder 负责向量化。
#[derive(Clone)]
pub struct MemoryStore {
    #[allow(dead_code)]
    config: MemoryConfig,
    pg: PgStore,
    neo4j: Neo4jStore,
    embedder: Arc<dyn Embedder + Send + Sync>,
}

impl MemoryStore {
    pub fn new(
        config: MemoryConfig,
        pg: PgStore,
        neo4j: Neo4jStore,
        embedder: Arc<dyn Embedder + Send + Sync>,
    ) -> Self {
        Self {
            config,
            pg,
            neo4j,
            embedder,
        }
    }

    pub async fn add(&mut self, memory_item: MemoryItem) -> Result<(), String> {
        let embedding = self.embedder
            .encode(&memory_item.content)
            .await
            .map_err(|e| format!("[MemoryStore] embedding calc failed: {}", e))?;

        self.pg.add(memory_item, embedding).await?;
        Ok(())
    }

    /// 添加记忆，并同时写入 Neo4j 实体引用图。
    ///
    /// `entities` / `relations` 由内部的实体抽取子 agent 从 content 中抽取，
    /// 每个 entity 只含 name/type，relation 只含 from_name/from_type/to_name/to_type/type，
    /// 业务层会根据 name+type 计算稳定的 entity id，memory_id / user_id / memory_type 会由本方法补全。
    pub async fn add_with_reference_graph(
        &mut self,
        memory_item: MemoryItem,
        entities: Vec<EntityInput>,
        relations: Vec<RelationInput>,
    ) -> Result<(), String> {
        if entities.is_empty() {
            return self.add(memory_item).await;
        }

        let embedding = self.embedder
            .encode(&memory_item.content)
            .await
            .map_err(|e| format!("[MemoryStore] embedding calc failed: {}", e))?;

        let memory_id = memory_item.id.clone();
        let user_id = memory_item.user_id.clone();
        let memory_type = memory_item.memory_type.clone();

        self.pg.add(memory_item, embedding).await?;

        let mut id_map: HashMap<(String, String), String> = HashMap::new();
        let entity_refs: Vec<neo4j::Entity> = entities
            .into_iter()
            .map(|e| {
                let id = entity_id(&e.name, &e.entity_type);
                id_map.insert((e.name.clone(), e.entity_type.clone()), id.clone());
                neo4j::Entity {
                    id,
                    name: e.name,
                    entity_type: e.entity_type,
                }
            })
            .collect();

        let relation_refs: Vec<neo4j::Relation> = relations
            .into_iter()
            .map(|r| {
                let from_id = id_map
                    .get(&(r.from_name.clone(), r.from_entity_type.clone()))
                    .cloned()
                    .unwrap_or_else(|| entity_id(&r.from_name, &r.from_entity_type));
                let to_id = id_map
                    .get(&(r.to_name.clone(), r.to_entity_type.clone()))
                    .cloned()
                    .unwrap_or_else(|| entity_id(&r.to_name, &r.to_entity_type));
                neo4j::Relation {
                    from_id,
                    to_id,
                    relation_type: r.relation_type,
                    memory_id: memory_id.clone(),
                    user_id: user_id.clone(),
                }
            })
            .collect();

        self.neo4j
            .add_reference_graph(
                &memory_id,
                &user_id,
                &memory_type,
                &entity_refs,
                &relation_refs,
            )
            .await?;

        Ok(())
    }

    pub async fn search_similar(
        &self,
        query: &str,
        memory_type: &str,
        user_id: Option<&str>,
        session_id: Option<&str>,
        importance_threshold: Option<f64>,
        time_range: Option<(i64, i64)>,
        limit: usize,
    ) -> Result<Vec<(f64, MemoryItem)>, String> {
        let embedding = self.embedder
            .encode(query)
            .await
            .map_err(|e| format!("[MemoryStore] embedding calc failed: {}", e))?;
        let pg_vector = Vector::from(embedding);

        self.pg.search_similar(
            pg_vector,
            memory_type,
            user_id,
            session_id,
            importance_threshold,
            time_range,
            limit,
        ).await
    }

    pub async fn keyword_search(
        &self,
        query: &str,
        memory_type: &str,
        user_id: Option<&str>,
        session_id: Option<&str>,
        importance_threshold: Option<f64>,
        time_range: Option<(i64, i64)>,
    ) -> Result<Vec<MemoryItem>, String> {
        self.pg.keyword_search(
            query,
            memory_type,
            user_id,
            session_id,
            importance_threshold,
            time_range,
        ).await
    }

    pub async fn get(&self, memory_id: &str) -> Result<Option<MemoryItem>, String> {
        self.pg.get(memory_id).await
    }

    pub async fn update(
        &self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f64>,
        metadata: Option<&Value>,
    ) -> Result<bool, String> {
        let new_embedding = match content {
            Some(text) => {
                let embedding = self.embedder
                    .encode(text)
                    .await
                    .map_err(|e| format!("[MemoryStore] update embedding failed: {}", e))?;
                Some(Vector::from(embedding))
            }
            None => None,
        };

        self.pg.update(memory_id, content, importance, metadata, new_embedding).await
    }

    pub async fn delete(&self, memory_id: &str) -> Result<bool, String> {
        let pg_ok = self.pg.delete(memory_id).await?;
        let _ = self.neo4j.delete_reference_graph_by_memory(memory_id).await;
        Ok(pg_ok)
    }

    pub async fn list_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<MemoryItem>, String> {
        self.pg.list_by_type(memory_type, user_id, limit).await
    }

    pub async fn clear_by_type(&self, memory_type: &str) -> Result<u64, String> {
        self.pg.clear_by_type(memory_type).await
    }

    pub async fn count_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<i64, String> {
        self.pg.count_by_type(memory_type, user_id).await
    }

    pub async fn avg_importance_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, String> {
        self.pg.avg_importance_by_type(memory_type, user_id).await
    }

    pub async fn time_span_days_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, String> {
        self.pg.time_span_days_by_type(memory_type, user_id).await
    }

    pub async fn get_related(
        &self,
        memory_id: &str,
        depth: i64,
        limit: usize,
    ) -> Result<Vec<MemoryItem>, String> {
        // 先拿到源记忆的 user_id，保证只查询该用户下的引用图。
        let source = self
            .pg
            .get(memory_id)
            .await?
            .ok_or_else(|| format!("[MemoryStore] source memory {} not found", memory_id))?;

        let related_ids = self
            .neo4j
            .get_related_memory_ids(memory_id, &source.user_id, depth, limit * 5)
            .await?;

        let mut items = Vec::new();
        for id in related_ids.into_iter().take(limit) {
            if let Some(item) = self.pg.get(&id).await? {
                items.push(item);
            }
        }
        Ok(items)
    }
}

/// 根据实体 name + type 计算稳定的 entity id。
///
/// 使用 64-bit FNV-1a 算法，保证跨进程、跨运行得到的 hash 一致，
/// 从而让相同 name+type 的实体在 Neo4j 中对应同一个 `:Entity` 节点。
pub fn entity_id(name: &str, entity_type: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let key = format!("{}:{}", name, entity_type);
    let mut hash = FNV_OFFSET;
    for byte in key.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", hash)
}
