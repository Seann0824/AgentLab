use serde_json::Value;
use std::{collections::HashMap, env, sync::Arc};

use crate::db::get_db_client;
use crate::tools::memory::base::Memory as MemoryTrait;
use crate::tools::memory::base::{MemoryConfig, MemoryItem, RetrieveRequest};
use crate::tools::memory::episodic_memory::EpisodicMemory;
use crate::tools::memory::extractor::EntityExtractorAgent;
use crate::tools::memory::perceptual_memory::PerceptualMemory;
use crate::tools::memory::semantic_memory::SemanticMemory;
use crate::tools::memory::storage::{MemoryStore, Neo4jStore, OllamaEmbedder, PgStore};
use crate::tools::memory::working_memory::WorkingMemory;

pub struct MemoryManager {
    #[allow(dead_code)]
    config: MemoryConfig,
    user_id: String,
    store: MemoryStore,
    memory_types: HashMap<String, Box<dyn MemoryTrait>>,
    extractor: EntityExtractorAgent,
}

impl MemoryManager {
    pub async fn new(
        config: Option<MemoryConfig>,
        user_id: Option<String>,
        enable_working: Option<bool>,
        enable_episodic: Option<bool>,
        enable_semantic: Option<bool>,
        enable_perceptual: Option<bool>,
    ) -> Self {
        let user_id = user_id.unwrap_or("default_user".into());
        let config = config.unwrap_or(MemoryConfig::new());
        let enable_working = enable_working.unwrap_or(true);
        let enable_episodic = enable_episodic.unwrap_or(true);
        let enable_semantic = enable_semantic.unwrap_or(true);
        let enable_perceptual = enable_perceptual.unwrap_or(true);

        let db = get_db_client().await;
        let pg_store = PgStore::new(config.clone(), db);

        let neo4j_uri = env::var("NEO4J_URL").unwrap_or_else(|_| "neo4j://127.0.0.1:7687".into());
        let neo4j_user = env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
        let neo4j_password = env::var("NEO4J_PASSWORD").unwrap_or_default();
        let neo4j_store = Neo4jStore::new(neo4j_uri, neo4j_user, neo4j_password)
            .await
            .expect("[MemoryManager] neo4j connection failed");

        let embedder = OllamaEmbedder::new(None, None);
        let store = MemoryStore::new(config.clone(), pg_store, neo4j_store, Arc::new(embedder));
        let extractor = EntityExtractorAgent::from_env();
        let semantic_extractor = EntityExtractorAgent::from_env();

        let mut memory_types: HashMap<String, Box<dyn MemoryTrait>> = HashMap::new();

        if enable_working {
            memory_types.insert(
                "working".into(),
                Box::new(WorkingMemory::new(config.clone(), store.clone())),
            );
        }
        if enable_episodic {
            memory_types.insert(
                "episodic".into(),
                Box::new(EpisodicMemory::new(config.clone(), store.clone())),
            );
        }
        if enable_semantic {
            memory_types.insert(
                "semantic".into(),
                Box::new(SemanticMemory::new(
                    config.clone(),
                    store.clone(),
                    semantic_extractor,
                )),
            );
        }
        if enable_perceptual {
            memory_types.insert(
                "perceptual".into(),
                Box::new(PerceptualMemory::new(config.clone(), store.clone())),
            );
        }

        Self {
            config,
            user_id,
            store,
            memory_types,
            extractor,
        }
    }

    pub async fn add_memory(
        &mut self,
        content: String,
        memory_type: String,
        importance: f32,
        metadata: Value,
        auto_classify: bool,
    ) -> Result<String, String> {
        let memory_item = MemoryItem::new(
            self.user_id.clone(),
            memory_type.clone(),
            content.clone(),
            importance as f64,
            metadata,
        );
        let memory_id = memory_item.id.clone();

        let target_type = if auto_classify {
            // 简单自动分类：后续可扩展为根据内容选择最合适的记忆类型
            memory_type.clone()
        } else {
            memory_type.clone()
        };

        let Some(memory_store) = self.memory_types.get_mut(&target_type) else {
            return Err(format!("记忆类型 {} 不存在", target_type));
        };

        if target_type == "semantic" {
            // 语义记忆自己负责实体抽取、metadata 标记和 Neo4j 引用图写入。
            memory_store.add(memory_item).await;
            return Ok(memory_id);
        }

        // 其他类型：由 MemoryManager 统一抽取实体/关系，抽成功且非空则写 Neo4j 引用图。
        match self.extractor.extract(&content).await {
            Ok((entities, relations)) if !entities.is_empty() => {
                self.store
                    .add_with_reference_graph(memory_item, entities, relations)
                    .await?;
            }
            Ok(_) => {
                self.store.add(memory_item).await?;
            }
            Err(e) => {
                tracing::warn!(
                    "[MemoryManager] entity extraction failed: {}, fallback to pg only",
                    e
                );
                self.store.add(memory_item).await?;
            }
        }

        Ok(memory_id)
    }

    pub async fn retrieve_memories(
        &mut self,
        query: &str,
        limit: usize,
        memory_types: &Vec<String>,
        min_importance: f32,
    ) -> Result<Vec<MemoryItem>, String> {
        let query_owned = query.to_string();
        let mut all_results = vec![];

        let types_to_search: Vec<String> = if memory_types.is_empty() {
            self.memory_types.keys().cloned().collect()
        } else {
            memory_types.clone()
        };

        for memory_type in &types_to_search {
            let Some(memory_store) = self.memory_types.get_mut(memory_type) else {
                continue;
            };
            let request = RetrieveRequest {
                query: query_owned.clone(),
                limit: Some(limit),
                user_id: Some(self.user_id.clone()),
                importance_threshold: Some(min_importance as f64),
                ..Default::default()
            };
            let results = memory_store.retrieve(request).await;
            all_results.extend(results);
        }

        let min_importance = min_importance as f64;
        all_results.retain(|m| m.importance >= min_importance);
        all_results.sort_by(|a, b| b.importance.total_cmp(&a.importance));
        all_results.truncate(limit);

        Ok(all_results)
    }

    pub async fn forget_by_type(
        &self,
        memory_type: &str,
        strategy: &str,
        threshold: f32,
        max_age_days: i64,
    ) -> Result<usize, String> {
        let Some(memory_store) = self.memory_types.get(memory_type) else {
            return Err(format!("记忆类型 {} 不存在", memory_type));
        };

        memory_store
            .forget(strategy, threshold as f64, max_age_days)
            .await
    }

    pub async fn consolidate_memories(
        &mut self,
        _from_type: &String,
        _to_type: &String,
        _importance_threshold: f32,
    ) -> Result<usize, String> {
        // TODO: 实现真正的记忆整合（如 working → episodic 的聚合/摘要）
        Ok(0)
    }

    pub async fn update_memory(
        &mut self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f32>,
        metadata: Option<Value>,
    ) -> Result<bool, String> {
        // store.update 通过 memory_id 直接更新 PG 中的行，对全部记忆类型通用。
        self.store
            .update(
                memory_id,
                content,
                importance.map(|v| v as f64),
                metadata.as_ref(),
            )
            .await
    }

    pub async fn remove_memory(&mut self, memory_id: &str) -> Result<bool, String> {
        self.store.delete(memory_id).await
    }

    pub async fn get_summary(&self, memory_type: &str, limit: usize) -> Result<String, String> {
        let items = self
            .store
            .list_by_type(memory_type, Some(&self.user_id), Some(limit as i64))
            .await?;

        if items.is_empty() {
            return Ok(format!("{} 类型下暂无记忆", memory_type));
        }

        let lines: Vec<String> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let preview = if item.content.len() > 80 {
                    format!("{} ...", item.content.chars().take(80).collect::<String>())
                } else {
                    item.content.clone()
                };
                format!("{}. {}", i + 1, preview)
            })
            .collect();

        Ok(format!(
            "{} 类型前 {} 条记忆摘要：\n{}",
            memory_type,
            lines.len(),
            lines.join("\n")
        ))
    }

    pub async fn get_stats(&self, memory_type: &str) -> Result<String, String> {
        let count = self.store.count_by_type(memory_type, Some(&self.user_id)).await?;
        let avg_importance = self
            .store
            .avg_importance_by_type(memory_type, Some(&self.user_id))
            .await?
            .unwrap_or(0.0);
        let time_span_days = self
            .store
            .time_span_days_by_type(memory_type, Some(&self.user_id))
            .await?
            .unwrap_or(0.0);

        let stats = serde_json::json!({
            "memory_type": memory_type,
            "count": count,
            "avg_importance": avg_importance,
            "time_span_days": time_span_days,
        });

        serde_json::to_string_pretty(&stats)
            .map_err(|e| format!("[MemoryManager] serialize stats failed: {}", e))
    }

    pub async fn clear_all(&mut self, memory_type: Option<&str>) -> Result<u64, String> {
        match memory_type {
            Some(t) => self.store.clear_by_type(t).await,
            None => {
                let mut total = 0u64;
                for t in self.memory_types.keys() {
                    total += self.store.clear_by_type(t).await?;
                }
                Ok(total)
            }
        }
    }
}
