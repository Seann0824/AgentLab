use chrono::Local;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use crate::base::llm::AgentsLLM;
use crate::db::get_db_client;
use crate::error::AgentLabError;
use crate::services::ServiceError;
use crate::storage::{MemoryStore, Neo4jStore, OllamaEmbedder, PgStore};
use crate::tools::memory::base::Memory as MemoryTrait;
use crate::tools::memory::base::{MemoryConfig, MemoryItem, RetrieveRequest};
use crate::tools::memory::episodic_memory::EpisodicMemory;
use crate::tools::memory::extractor::EntityExtractorAgent;
use crate::tools::memory::perceptual_memory::PerceptualMemory;
use crate::tools::memory::semantic_memory::SemanticMemory;
use crate::tools::memory::working_memory::WorkingMemory;

/// 记忆业务服务：面向应用层提供记忆 CRUD、搜索、统计、整合等能力。
pub struct MemoryService {
    #[allow(dead_code)]
    config: MemoryConfig,
    user_id: String,
    store: MemoryStore,
    memory_types: HashMap<String, Box<dyn MemoryTrait>>,
    extractor: EntityExtractorAgent,
    current_session_id: Option<String>,
}

impl MemoryService {
    pub async fn new(
        config: Option<MemoryConfig>,
        user_id: Option<String>,
        llm: AgentsLLM,
        database_url: impl Into<String>,
        neo4j_uri: impl Into<String>,
        neo4j_user: impl Into<String>,
        neo4j_password: impl Into<String>,
        enable_working: Option<bool>,
        enable_episodic: Option<bool>,
        enable_semantic: Option<bool>,
        enable_perceptual: Option<bool>,
    ) -> Result<Self, AgentLabError> {
        let user_id = user_id.unwrap_or("default_user".into());
        let config = config.unwrap_or(MemoryConfig::new());
        let enable_working = enable_working.unwrap_or(true);
        let enable_episodic = enable_episodic.unwrap_or(true);
        let enable_semantic = enable_semantic.unwrap_or(true);
        let enable_perceptual = enable_perceptual.unwrap_or(true);

        let db = get_db_client(&database_url.into()).await;
        let pg_store = PgStore::new(config.clone(), db);

        let neo4j_store =
            Neo4jStore::new(neo4j_uri.into(), neo4j_user.into(), neo4j_password.into()).await?;

        let embedder = OllamaEmbedder::new(None, None);
        let store = MemoryStore::new(config.clone(), pg_store, neo4j_store, Arc::new(embedder));
        let extractor = EntityExtractorAgent::new(llm.clone());
        let semantic_extractor = EntityExtractorAgent::new(llm);

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

        Ok(Self {
            config,
            user_id,
            store,
            memory_types,
            extractor,
            current_session_id: None,
        })
    }

    /// 添加一条记忆。
    /// 若未提供 session_id，会自动分配当前会话 id 并写入 metadata。
    pub async fn add_memory(
        &mut self,
        content: String,
        memory_type: String,
        importance: f32,
        metadata: Option<Value>,
    ) -> Result<String, AgentLabError> {
        if self.current_session_id.is_none() {
            self.current_session_id =
                Some(format!("session_{}", Local::now().format("%Y%m%d_%H%M%S")));
        }

        let mut metadata = metadata.unwrap_or_else(|| serde_json::json!({}));
        metadata["session_id"] = Value::from(self.current_session_id.clone());
        metadata["timestamp"] = Value::from(Local::now().to_string());

        let memory_item = MemoryItem::new(
            self.user_id.clone(),
            memory_type.clone(),
            content.clone(),
            importance as f64,
            metadata,
        );
        let memory_id = memory_item.id.clone();

        let Some(memory_store) = self.memory_types.get_mut(&memory_type) else {
            return Err(ServiceError::invalid_argument(format!(
                "记忆类型 {} 不存在",
                memory_type
            )))?;
        };

        if memory_type == "semantic" {
            // 语义记忆自己负责实体抽取、metadata 标记和 Neo4j 引用图写入。
            memory_store.add(memory_item).await;
            return Ok(memory_id);
        }

        // 其他类型：统一抽取实体/关系，抽成功且非空则写 Neo4j 引用图。
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
                    "[MemoryService] entity extraction failed: {}, fallback to pg only",
                    e
                );
                self.store.add(memory_item).await?;
            }
        }

        Ok(memory_id)
    }

    pub async fn search_memories(
        &mut self,
        query: &str,
        limit: usize,
        memory_types: &[String],
        min_importance: f32,
    ) -> Result<Vec<MemoryItem>, AgentLabError> {
        let query_owned = query.to_string();
        let mut all_results = vec![];

        let types_to_search: Vec<String> = if memory_types.is_empty() {
            self.memory_types.keys().cloned().collect()
        } else {
            memory_types.to_vec()
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
    ) -> Result<usize, AgentLabError> {
        let Some(memory_store) = self.memory_types.get(memory_type) else {
            return Err(ServiceError::invalid_argument(format!(
                "记忆类型 {} 不存在",
                memory_type
            )))?;
        };

        let count = memory_store
            .forget(strategy, threshold as f64, max_age_days)
            .await?;
        Ok(count)
    }

    pub async fn consolidate_memories(
        &mut self,
        _from_type: &str,
        _to_type: &str,
        _importance_threshold: f32,
    ) -> Result<usize, AgentLabError> {
        // TODO: 实现真正的记忆整合（如 working → episodic 的聚合/摘要）
        Ok(0)
    }

    pub async fn update_memory(
        &mut self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f32>,
        metadata: Option<Value>,
    ) -> Result<bool, AgentLabError> {
        let ok = self
            .store
            .update(
                memory_id,
                content,
                importance.map(|v| v as f64),
                metadata.as_ref(),
            )
            .await?;
        Ok(ok)
    }

    pub async fn remove_memory(&mut self, memory_id: &str) -> Result<bool, AgentLabError> {
        let ok = self.store.delete(memory_id).await?;
        Ok(ok)
    }

    pub async fn get_summary(
        &self,
        memory_type: &str,
        limit: usize,
    ) -> Result<String, AgentLabError> {
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
            .map(|(i, item)| format!("{}. {}", i + 1, item.content))
            .collect();

        Ok(format!(
            "{} 类型前 {} 条记忆摘要：\n{}",
            memory_type,
            lines.len(),
            lines.join("\n")
        ))
    }

    pub async fn get_stats(&self, memory_type: &str) -> Result<String, AgentLabError> {
        let count = self
            .store
            .count_by_type(memory_type, Some(&self.user_id))
            .await?;
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

        serde_json::to_string_pretty(&stats).map_err(|e| AgentLabError::Serialization(e))
    }

    pub async fn clear_all(&mut self, memory_type: Option<&str>) -> Result<u64, AgentLabError> {
        match memory_type {
            Some(t) => {
                let count = self.store.clear_by_type(t).await?;
                Ok(count)
            }
            None => {
                let mut total = 0u64;
                for t in self.memory_types.keys() {
                    total += self.store.clear_by_type(t).await?;
                }
                Ok(total)
            }
        }
    }

    // === 面向 Agent 的便捷方法：参数校验、默认值、结果格式化统一放在 Service ===

    pub async fn add_memory_agent(
        &mut self,
        content: Option<&str>,
        memory_type: Option<&str>,
        importance: Option<f32>,
    ) -> Result<String, AgentLabError> {
        let content = content.ok_or_else(|| ServiceError::invalid_argument("content 不能为空"))?;
        if content.is_empty() {
            return Err(ServiceError::invalid_argument("content 不能为空"))?;
        }
        let memory_type = memory_type.unwrap_or("working").to_string();
        let importance = importance.unwrap_or(0.5);
        let id = self.add_memory(content.into(), memory_type, importance, None).await?;
        Ok(format!("记忆已添加（ID: {}）", id))
    }

    pub async fn search_memories_agent(
        &mut self,
        query: Option<&str>,
        memory_type: Option<&str>,
        limit: Option<u64>,
    ) -> Result<String, AgentLabError> {
        let query = query.ok_or_else(|| ServiceError::invalid_argument("query 不能为空"))?;
        if query.is_empty() {
            return Err(ServiceError::invalid_argument("query 不能为空"))?;
        }
        let limit = limit.unwrap_or(5) as usize;
        let memory_types: Vec<String> = memory_type.map(|t| vec![t.into()]).unwrap_or_default();

        let results = self.search_memories(query, limit, &memory_types, 0.1).await?;
        if results.is_empty() {
            return Ok(format!("未找到与 {} 相关的记忆", query));
        }

        let mut formatted = vec![format!("找到 {} 条相关记忆", results.len())];
        for (i, memory) in results.iter().enumerate() {
            let label = memory_type_label(&memory.memory_type);
            formatted.push(format!(
                "{}. [{}] {} (重要性: {})",
                i + 1,
                label,
                memory.content,
                memory.importance
            ));
        }
        Ok(formatted.join("\n"))
    }

    pub async fn update_memory_agent(
        &mut self,
        memory_id: Option<&str>,
        content: Option<&str>,
        importance: Option<f32>,
        metadata: Option<Value>,
    ) -> Result<String, AgentLabError> {
        let memory_id =
            memory_id.ok_or_else(|| ServiceError::invalid_argument("memory_id 不能为空"))?;
        if memory_id.is_empty() {
            return Err(ServiceError::invalid_argument("memory_id 不能为空"))?;
        }
        let ok = self
            .update_memory(memory_id, content, importance, metadata)
            .await?;
        if ok {
            Ok(format!("记忆 {} 更新成功", memory_id))
        } else {
            Ok(format!("未找到记忆 {}", memory_id))
        }
    }

    pub async fn remove_memory_agent(
        &mut self,
        memory_id: Option<&str>,
    ) -> Result<String, AgentLabError> {
        let memory_id =
            memory_id.ok_or_else(|| ServiceError::invalid_argument("memory_id 不能为空"))?;
        if memory_id.is_empty() {
            return Err(ServiceError::invalid_argument("memory_id 不能为空"))?;
        }
        let ok = self.remove_memory(memory_id).await?;
        if ok {
            Ok(format!("记忆 {} 已删除", memory_id))
        } else {
            Ok(format!("未找到记忆 {}", memory_id))
        }
    }

    pub async fn forget_by_type_agent(
        &self,
        memory_type: Option<&str>,
        strategy: Option<&str>,
        threshold: Option<f32>,
        max_age_days: Option<u64>,
    ) -> Result<String, AgentLabError> {
        let memory_type = memory_type.unwrap_or("working");
        let strategy = strategy.unwrap_or("importance_based");
        let threshold = threshold.unwrap_or(0.1);
        let max_age_days = max_age_days.unwrap_or(30) as i64;
        let count = self
            .forget_by_type(memory_type, strategy, threshold, max_age_days)
            .await?;
        Ok(format!(
            "已遗忘 {} 条 {} 记忆（策略: {}）",
            count, memory_type, strategy
        ))
    }

    pub async fn consolidate_memories_agent(
        &mut self,
        from_type: Option<&str>,
        to_type: Option<&str>,
        importance_threshold: Option<f32>,
    ) -> Result<String, AgentLabError> {
        let from_type = from_type.unwrap_or("working");
        let to_type = to_type.unwrap_or("episodic");
        let importance_threshold = importance_threshold.unwrap_or(0.7);
        let count = self
            .consolidate_memories(from_type, to_type, importance_threshold)
            .await?;
        Ok(format!(
            "已整合 {} 条记忆为长期记忆（{} → {}，阈值={}）",
            count, from_type, to_type, importance_threshold
        ))
    }

    pub async fn clear_all_agent(
        &mut self,
        memory_type: Option<&str>,
    ) -> Result<String, AgentLabError> {
        let count = self.clear_all(memory_type).await?;
        Ok(format!("已清空 {} 条记忆", count))
    }

    pub async fn summary_agent(
        &self,
        memory_type: Option<&str>,
        limit: Option<u64>,
    ) -> Result<String, AgentLabError> {
        let memory_type = memory_type.unwrap_or("working");
        let limit = limit.unwrap_or(5) as usize;
        self.get_summary(memory_type, limit).await
    }

    pub async fn stats_agent(&self, memory_type: Option<&str>) -> Result<String, AgentLabError> {
        let memory_type = memory_type.unwrap_or("working");
        self.get_stats(memory_type).await
    }
}

fn memory_type_label(memory_type: &str) -> &'static str {
    match memory_type {
        "working" => "工作记忆",
        "episodic" => "情景记忆",
        "semantic" => "语义记忆",
        "perceptual" => "感知记忆",
        _ => "未知类型",
    }
}
