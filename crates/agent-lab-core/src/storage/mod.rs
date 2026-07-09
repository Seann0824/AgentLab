pub mod embedder;
pub mod error;
pub mod graph;
pub mod neo4j;
pub mod pg;

pub use embedder::{Embedder, OllamaEmbedder};
pub use error::StorageError;
pub use graph::entity_id;
pub use neo4j::{Entity, EntityInput, Neo4jStore, Relation, RelationInput};
pub use pg::PgStore;

use std::collections::HashMap;
use std::sync::Arc;

use pgvector::Vector;
use serde_json::Value;

use crate::tools::memory::base::{MemoryConfig, MemoryItem};

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

    pub async fn add(&mut self, memory_item: MemoryItem) -> Result<(), StorageError> {
        let embedding = self
            .embedder
            .encode(&memory_item.content)
            .await
            .map_err(|e| StorageError::embedding(format!("[MemoryStore] embedding calc failed: {}", e)))?;

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
    ) -> Result<(), StorageError> {
        if entities.is_empty() {
            return self.add(memory_item).await;
        }

        let embedding = self
            .embedder
            .encode(&memory_item.content)
            .await
            .map_err(|e| StorageError::embedding(format!("[MemoryStore] embedding calc failed: {}", e)))?;

        let memory_id = memory_item.id.clone();
        let user_id = memory_item.user_id.clone();
        let memory_type = memory_item.memory_type.clone();

        self.pg.add(memory_item, embedding).await?;

        // 1) 先收集显式抽取出的实体，并计算稳定 id。
        let mut id_map: HashMap<(String, String), String> = HashMap::new();
        let mut entity_map: HashMap<(String, String), neo4j::Entity> = HashMap::new();
        for e in entities {
            let id = entity_id(&e.name, &e.entity_type);
            let key = (e.name.clone(), e.entity_type.clone());
            id_map.insert(key.clone(), id.clone());
            entity_map.insert(
                key,
                neo4j::Entity {
                    id,
                    name: e.name,
                    entity_type: e.entity_type,
                },
            );
        }

        // 2) 关系里引用的实体可能未被 LLM 单独抽出来，必须补全到 entity_map 中，
        //    否则 Neo4j 里 MATCH 不到节点，关系会静默建不上。
        for r in &relations {
            let from_key = (r.from_name.clone(), r.from_entity_type.clone());
            if !entity_map.contains_key(&from_key) {
                let id = entity_id(&r.from_name, &r.from_entity_type);
                id_map.insert(from_key.clone(), id.clone());
                entity_map.insert(
                    from_key,
                    neo4j::Entity {
                        id,
                        name: r.from_name.clone(),
                        entity_type: r.from_entity_type.clone(),
                    },
                );
            }
            let to_key = (r.to_name.clone(), r.to_entity_type.clone());
            if !entity_map.contains_key(&to_key) {
                let id = entity_id(&r.to_name, &r.to_entity_type);
                id_map.insert(to_key.clone(), id.clone());
                entity_map.insert(
                    to_key,
                    neo4j::Entity {
                        id,
                        name: r.to_name.clone(),
                        entity_type: r.to_entity_type.clone(),
                    },
                );
            }
        }

        let entity_refs: Vec<neo4j::Entity> = entity_map.into_values().collect();

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
    ) -> Result<Vec<(f64, MemoryItem)>, StorageError> {
        let embedding = self
            .embedder
            .encode(query)
            .await
            .map_err(|e| StorageError::embedding(format!("[MemoryStore] embedding calc failed: {}", e)))?;
        let pg_vector = Vector::from(embedding);

        self.pg
            .search_similar(
                pg_vector,
                memory_type,
                user_id,
                session_id,
                importance_threshold,
                time_range,
                limit,
            )
            .await
    }

    pub async fn keyword_search(
        &self,
        query: &str,
        memory_type: &str,
        user_id: Option<&str>,
        session_id: Option<&str>,
        importance_threshold: Option<f64>,
        time_range: Option<(i64, i64)>,
    ) -> Result<Vec<MemoryItem>, StorageError> {
        self.pg
            .keyword_search(
                query,
                memory_type,
                user_id,
                session_id,
                importance_threshold,
                time_range,
            )
            .await
    }

    pub async fn get(&self, memory_id: &str) -> Result<Option<MemoryItem>, StorageError> {
        self.pg.get(memory_id).await
    }

    pub async fn update(
        &self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f64>,
        metadata: Option<&Value>,
    ) -> Result<bool, StorageError> {
        let new_embedding = match content {
            Some(text) => {
                let embedding = self
                    .embedder
                    .encode(text)
                    .await
                    .map_err(|e| StorageError::embedding(format!("[MemoryStore] update embedding failed: {}", e)))?;
                Some(Vector::from(embedding))
            }
            None => None,
        };

        self.pg
            .update(memory_id, content, importance, metadata, new_embedding)
            .await
    }

    pub async fn delete(&self, memory_id: &str) -> Result<bool, StorageError> {
        let pg_ok = self.pg.delete(memory_id).await?;
        let _ = self.neo4j.delete_reference_graph_by_memory(memory_id).await;
        Ok(pg_ok)
    }

    pub async fn list_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<MemoryItem>, StorageError> {
        self.pg.list_by_type(memory_type, user_id, limit).await
    }

    pub async fn clear_by_type(&self, memory_type: &str) -> Result<u64, StorageError> {
        let count = self.pg.clear_by_type(memory_type).await?;
        // PG 清空后，同步清空 Neo4j 中该类型的记忆引用图，避免两侧数据不一致。
        let _ = self
            .neo4j
            .delete_reference_graph_by_memory_type(memory_type)
            .await;
        Ok(count)
    }

    pub async fn count_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<i64, StorageError> {
        self.pg.count_by_type(memory_type, user_id).await
    }

    pub async fn avg_importance_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, StorageError> {
        self.pg.avg_importance_by_type(memory_type, user_id).await
    }

    pub async fn time_span_days_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, StorageError> {
        self.pg.time_span_days_by_type(memory_type, user_id).await
    }

    /// 根据一组实体 id 查找关联的记忆 id 及其命中实体数。
    ///
    /// 代理到 Neo4j，返回 `(memory_id, matched_count)`，供语义记忆做图检索分数。
    pub async fn get_memory_ids_by_entities(
        &self,
        user_id: &str,
        entity_ids: &[String],
        limit: usize,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        self.neo4j
            .get_memory_ids_by_entities(user_id, entity_ids, limit)
            .await
    }

    /// 根据一组实体 id，通过实体关系图查找关联记忆及其命中相关实体数。
    pub async fn get_related_memory_ids_by_entities(
        &self,
        user_id: &str,
        entity_ids: &[String],
        depth: i64,
        limit: usize,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        self.neo4j
            .get_related_memory_ids_by_entities(user_id, entity_ids, depth, limit)
            .await
    }

    pub async fn get_related(
        &self,
        memory_id: &str,
        depth: i64,
        limit: usize,
    ) -> Result<Vec<MemoryItem>, StorageError> {
        // 先拿到源记忆的 user_id，保证只查询该用户下的引用图。
        let source = self
            .pg
            .get(memory_id)
            .await?
            .ok_or_else(|| StorageError::NotFound)?;

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
