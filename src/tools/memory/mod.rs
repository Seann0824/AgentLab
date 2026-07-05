use chrono::Local;
use openai_api_rs::v1::types;
use serde_json::Value;
use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::Mutex;

use crate::tools::{memory::storage::OllamaEmbedder, types::Tool};
pub mod base;
pub mod episodic_memory;
pub mod extractor;
mod perceptual_memory;
pub mod semantic_memory;
pub mod storage;
mod working_memory;
pub use base::{Memory, MemoryConfig, MemoryItem, RetrieveRequest};
use base::{Memory as MemoryTrait, get_db_client};
use episodic_memory::EpisodicMemory;
use extractor::EntityExtractorAgent;
use perceptual_memory::PerceptualMemory;
use semantic_memory::SemanticMemory;
use storage::MemoryStore;
use working_memory::WorkingMemory;

pub struct MemoryTool {
    inner: Mutex<MemoryToolInner>,
}

struct MemoryToolInner {
    current_session_id: Option<String>,
    memory_manager: MemoryManager,
}

impl MemoryTool {
    pub async fn new() -> Self {
        Self {
            inner: Mutex::new(MemoryToolInner {
                current_session_id: None,
                memory_manager: MemoryManager::new(None, None, None, None, None, None).await,
            }),
        }
    }

    async fn add_memory(
        &self,
        content: String,
        memory_type: String,
        importance: Option<f32>,
        _file_path: Option<String>,
        _modality: Option<String>,
        metadata: impl Into<Option<Value>>,
    ) -> String {
        let importance = importance.unwrap_or(0.5f32);
        let mut inner = self.inner.lock().await;

        // 没有则分配会话id
        if inner.current_session_id.is_none() {
            inner.current_session_id =
                Some(format!("session_{}", Local::now().format("%Y%m%d_%H%M%S"),));
        }

        let mut metadata: Value = metadata.into().unwrap_or_else(|| serde_json::json!({}));

        // 添加会话信息到元数据
        metadata["session_id"] = Value::from(inner.current_session_id.clone());
        metadata["timestamp"] = Value::from(Local::now().to_string());

        let memory_id = inner
            .memory_manager
            .add_memory(content, memory_type, importance, metadata, false)
            .await;

        match memory_id {
            Ok(id) => format!("记忆已添加 （ID: {}）", id),
            Err(e) => format!("记忆添加失败: {}", e),
        }
    }

    async fn search_memory(
        &self,
        query: String,
        limit: Option<usize>,
        memory_types: Option<Vec<String>>,
        memory_type: Option<String>,
        min_importance: Option<f32>,
    ) -> String {
        let min_importance = min_importance.unwrap_or(0.1f32);
        let mut memory_types = memory_types.unwrap_or_default();
        let limit = limit.unwrap_or(5usize);

        if memory_type.is_some() && memory_types.is_empty() {
            memory_types.push(memory_type.unwrap().clone());
        }

        let mut inner = self.inner.lock().await;
        let results = inner
            .memory_manager
            .retrieve_memories(&query, limit, &memory_types, min_importance)
            .await;

        match results {
            Ok(results) => {
                if results.is_empty() {
                    return format!("未找到与 {} 相关的记忆", query);
                }

                let mut formatted_results = vec![];
                formatted_results.push(format!("找到 {} 条相关记忆", results.len()));

                for (i, memory) in results.iter().enumerate() {
                    let type_label_map = HashMap::from([
                        ("working", "工作记忆"),
                        ("episodic", "情景记忆"),
                        ("semantic", "语义记忆"),
                        ("perceptual", "感知记忆"),
                    ]);
                    let memory_type_label = type_label_map
                        .get(memory.memory_type.as_str())
                        .unwrap_or(&"未知类型");
                    let content_preview = if memory.content.len() > 80usize {
                        format!(
                            "{} ...",
                            memory.content.chars().take(80).collect::<String>()
                        )
                    } else {
                        memory.content.clone()
                    };

                    formatted_results.push(format!(
                        "{}. [{}] {} (重要性: {})",
                        i + 1,
                        memory_type_label,
                        content_preview,
                        memory.importance
                    ));
                }

                formatted_results.join("\n")
            }
            Err(msg) => format!("搜索记忆失败：{}", msg),
        }
    }

    // 短期记忆提升为长期记忆
    async fn consolidate(
        &self,
        from_type: Option<String>,
        to_type: Option<String>,
        importance_threshold: Option<f32>,
    ) -> String {
        let from_type = from_type.unwrap_or("working".to_string());
        let to_type = to_type.unwrap_or("episodic".to_string());
        let importance_threshold = importance_threshold.unwrap_or(0.7);
        let mut inner = self.inner.lock().await;
        match inner
            .memory_manager
            .consolidate_memories(&from_type, &to_type, importance_threshold)
            .await
        {
            Ok(count) => format!(
                "已整合 {} 条记忆为长期记忆（{} → {}，阈值={}）",
                count, from_type, to_type, importance_threshold
            ),
            Err(msg) => format!("整合记忆失败: {}", msg),
        }
    }

    async fn summary_memory(&self, memory_type: Option<String>, limit: Option<usize>) -> String {
        let memory_type = memory_type.unwrap_or_else(|| "working".to_string());
        let limit = limit.unwrap_or(5);
        let inner = self.inner.lock().await;
        match inner.memory_manager.get_summary(&memory_type, limit).await {
            Ok(summary) => summary,
            Err(msg) => format!("获取记忆摘要失败: {}", msg),
        }
    }

    async fn stats_memory(&self, memory_type: Option<String>) -> String {
        let memory_type = memory_type.unwrap_or_else(|| "working".to_string());
        let inner = self.inner.lock().await;
        match inner.memory_manager.get_stats(&memory_type).await {
            Ok(stats) => stats,
            Err(msg) => format!("获取记忆统计失败: {}", msg),
        }
    }

    async fn update_memory(
        &self,
        memory_id: String,
        content: Option<String>,
        importance: Option<f32>,
        metadata: Option<Value>,
    ) -> String {
        if memory_id.is_empty() {
            return "memory_id 不能为空".to_string();
        }
        let mut inner = self.inner.lock().await;
        match inner
            .memory_manager
            .update_memory(&memory_id, content.as_deref(), importance, metadata)
            .await
        {
            Ok(true) => format!("记忆 {} 更新成功", memory_id),
            Ok(false) => format!("未找到记忆 {}", memory_id),
            Err(msg) => format!("更新记忆失败: {}", msg),
        }
    }

    async fn remove_memory(&self, memory_id: String) -> String {
        if memory_id.is_empty() {
            return "memory_id 不能为空".to_string();
        }
        let mut inner = self.inner.lock().await;
        match inner.memory_manager.remove_memory(&memory_id).await {
            Ok(true) => format!("记忆 {} 已删除", memory_id),
            Ok(false) => format!("未找到记忆 {}", memory_id),
            Err(msg) => format!("删除记忆失败: {}", msg),
        }
    }

    async fn forget_memory(
        &self,
        memory_type: Option<String>,
        strategy: String,
        threshold: Option<f32>,
        max_age_days: Option<usize>,
    ) -> String {
        let memory_type = memory_type.unwrap_or_else(|| "working".to_string());
        let threshold = threshold.unwrap_or(0.1);
        let max_age_days = max_age_days.unwrap_or(30);
        let mut inner = self.inner.lock().await;
        match inner
            .memory_manager
            .forget_by_type(&memory_type, &strategy, threshold, max_age_days as i64)
            .await
        {
            Ok(count) => format!(
                "已遗忘 {} 条 {} 记忆（策略: {}）",
                count, memory_type, strategy
            ),
            Err(msg) => format!("遗忘记忆失败: {}", msg),
        }
    }

    async fn clear_all_memory(&self, memory_type: Option<String>) -> String {
        let mut inner = self.inner.lock().await;
        match inner.memory_manager.clear_all(memory_type.as_deref()).await {
            Ok(count) => format!("已清空 {} 条记忆", count),
            Err(msg) => format!("清空记忆失败: {}", msg),
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "记忆管理工具。支持 add(添加记忆), search(搜索记忆), summary(获取摘要), stats(获取统计), update(更新记忆), remove(删除记忆), forget(遗忘记忆), consolidate(整合记忆), clear_all(清空所有记忆)。"
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let properties = HashMap::from([
            (
                "action".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some(
                        "要执行的操作: add(添加记忆), search(搜索记忆), summary(获取摘要), stats(获取统计), update(更新记忆), remove(删除记忆), forget(遗忘记忆), consolidate(整合记忆), clear_all(清空所有记忆)".to_string(),
                    ),
                    enum_values: Some(vec![
                        "add".to_string(),
                        "search".to_string(),
                        "summary".to_string(),
                        "stats".to_string(),
                        "update".to_string(),
                        "remove".to_string(),
                        "forget".to_string(),
                        "consolidate".to_string(),
                        "clear_all".to_string(),
                    ]),
                    ..Default::default()
                }),
            ),
            (
                "content".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("add/update 时使用，要保存或更新的记忆内容".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "query".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("search 时必填，搜索关键词。为提高语义相似度，请把用户的疑问句转换成陈述句后再传入，例如：'我上周去了哪里？' -> '我上周去了'，'我和同事讨论了什么？' -> '我和同事讨论了'。".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "memory_type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("记忆类型：working, episodic, semantic, perceptual（默认：working）".to_string()),
                    enum_values: Some(vec![
                        "working".to_string(),
                        "episodic".to_string(),
                        "semantic".to_string(),
                        "perceptual".to_string(),
                    ]),
                    ..Default::default()
                }),
            ),
            (
                "memory_id".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("update/remove 时必需，目标记忆ID".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "importance".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("add/update 时使用，重要性 0.0-1.0，默认 0.5".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "limit".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("search/stats 时使用，返回条数上限，默认 5".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "file_path".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("感知记忆：本地文件路径（image/audio）".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "modality".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("感知记忆模态：text/image/audio（不传则按扩展名推断）".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "strategy".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("forget 时使用，遗忘策略：importance_based/time_based/capacity_based（默认 importance_based）".to_string()),
                    enum_values: Some(vec![
                        "importance_based".to_string(),
                        "time_based".to_string(),
                        "capacity_based".to_string(),
                    ]),
                    ..Default::default()
                }),
            ),
            (
                "threshold".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("forget 时使用，遗忘阈值（默认 0.1）".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "max_age_days".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("forget 策略为 time_based 时使用，最大保留天数（默认 30）".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "from_type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("consolidate 时使用，整合来源类型（默认 working）".to_string()),
                    enum_values: Some(vec![
                        "working".to_string(),
                        "episodic".to_string(),
                        "semantic".to_string(),
                        "perceptual".to_string(),
                    ]),
                    ..Default::default()
                }),
            ),
            (
                "to_type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("consolidate 时使用，整合目标类型（默认 episodic）".to_string()),
                    enum_values: Some(vec![
                        "working".to_string(),
                        "episodic".to_string(),
                        "semantic".to_string(),
                        "perceptual".to_string(),
                    ]),
                    ..Default::default()
                }),
            ),
            (
                "importance_threshold".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("consolidate 时使用，整合重要性阈值（默认 0.7）".to_string()),
                    ..Default::default()
                }),
            ),
        ]);
        openai_api_rs::v1::types::FunctionParameters {
            schema_type: openai_api_rs::v1::types::JSONSchemaType::Object,
            properties: Some(properties),
            required: Some(vec!["action".to_string()]),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().unwrap_or("");
        match action {
            "add" => {
                let content = args["content"].as_str().unwrap_or("").to_string();
                if content.is_empty() {
                    return Err("content 不能为空".into());
                }
                let memory_type = args["memory_type"]
                    .as_str()
                    .unwrap_or("working")
                    .to_string();
                let importance = args["importance"].as_f64().map(|v| v as f32);
                Ok(self
                    .add_memory(content, memory_type, importance, None, None, None)
                    .await)
            }
            "search" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                if query.is_empty() {
                    return Err("query 不能为空".into());
                }
                let limit = args["limit"].as_u64().map(|v| v as usize);
                let memory_type = args["memory_type"].as_str().map(|v| v.to_string());
                let memory_types = memory_type.map(|t| vec![t]);
                Ok(self
                    .search_memory(query, limit, memory_types, None, None)
                    .await)
            }
            "summary" => {
                let memory_type = args["memory_type"].as_str().map(|v| v.to_string());
                let limit = args["limit"].as_u64().map(|v| v as usize);
                Ok(self.summary_memory(memory_type, limit).await)
            }
            "stats" => {
                let memory_type = args["memory_type"].as_str().map(|v| v.to_string());
                Ok(self.stats_memory(memory_type).await)
            }
            "update" => {
                let memory_id = args["memory_id"].as_str().unwrap_or("").to_string();
                let content = args["content"].as_str().map(|v| v.to_string());
                let importance = args["importance"].as_f64().map(|v| v as f32);
                let metadata = args.get("metadata").cloned();
                Ok(self
                    .update_memory(memory_id, content, importance, metadata)
                    .await)
            }
            "remove" => {
                let memory_id = args["memory_id"].as_str().unwrap_or("").to_string();
                Ok(self.remove_memory(memory_id).await)
            }
            "forget" => {
                let memory_type = args["memory_type"].as_str().map(|v| v.to_string());
                let strategy = args["strategy"]
                    .as_str()
                    .unwrap_or("importance_based")
                    .to_string();
                let threshold = args["threshold"].as_f64().map(|v| v as f32);
                let max_age_days = args["max_age_days"].as_u64().map(|v| v as usize);
                Ok(self
                    .forget_memory(memory_type, strategy, threshold, max_age_days)
                    .await)
            }
            "consolidate" => {
                let from_type = args["from_type"].as_str().map(|v| v.to_string());
                let to_type = args["to_type"].as_str().map(|v| v.to_string());
                let importance_threshold = args["importance_threshold"].as_f64().map(|v| v as f32);
                Ok(self
                    .consolidate(from_type, to_type, importance_threshold)
                    .await)
            }
            "clear_all" => {
                let memory_type = args["memory_type"].as_str().map(|v| v.to_string());
                Ok(self.clear_all_memory(memory_type).await)
            }
            _ => Err(format!("不支持的 action: {}", action)),
        }
    }
}

pub struct MemoryManager {
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
        let pg_store = crate::tools::memory::storage::PgStore::new(config.clone(), db);

        let neo4j_uri = env::var("NEO4J_URL").unwrap_or_else(|_| "neo4j://127.0.0.1:7687".into());
        let neo4j_user = env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
        let neo4j_password = env::var("NEO4J_PASSWORD").unwrap_or_default();
        let neo4j_store =
            crate::tools::memory::storage::Neo4jStore::new(neo4j_uri, neo4j_user, neo4j_password)
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
                tracing::warn!("[MemoryManager] entity extraction failed: {}, fallback to pg only", e);
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

    pub async fn forget(
        &self,
        _strategy: &String,
        _threshold: f32,
        _max_age_days: usize,
    ) -> Result<usize, String> {
        Ok(0)
    }

    pub async fn forget_by_type(
        &mut self,
        memory_type: &str,
        strategy: &str,
        threshold: f32,
        max_age_days: i64,
    ) -> Result<usize, String> {
        let Some(_memory_store) = self.memory_types.get_mut(memory_type) else {
            return Err(format!("记忆类型 {} 不存在", memory_type));
        };

        // TODO: 根据 strategy 调用对应 Memory 实现的 forget 方法
        // 目前仅 episodic 实现了 forget，其他类型可先返回 0
        match memory_type {
            "episodic" => {
                // 这里需要把 Box<dyn Memory> 向下转型为 EpisodicMemory，暂时用 store 直接操作
                let _ = (strategy, threshold, max_age_days);
                Ok(0)
            }
            _ => Ok(0),
        }
    }

    pub async fn consolidate_memories(
        &mut self,
        _from_type: &String,
        _to_type: &String,
        _importance_threshold: f32,
    ) -> Result<usize, String> {
        Ok(0)
    }

    pub async fn update_memory(
        &mut self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f32>,
        metadata: Option<Value>,
    ) -> Result<bool, String> {
        // TODO: 需要知道 memory_id 对应的 memory_type 才能调用对应实现
        // 临时方案：遍历所有记忆类型尝试更新
        for memory_store in self.memory_types.values_mut() {
            // Memory trait 没有 update 方法，需要具体实现支持
            // 这里先用 store 层的通用 update
            let _ = (memory_id, content, importance, metadata.clone());
            let _ = memory_store;
        }
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
        let _ = (memory_type, limit);
        // TODO: 实现摘要生成
        Ok("记忆摘要功能待实现".to_string())
    }

    pub async fn get_stats(&self, memory_type: &str) -> Result<String, String> {
        let _ = memory_type;
        // TODO: 实现统计信息
        Ok("记忆统计功能待实现".to_string())
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
