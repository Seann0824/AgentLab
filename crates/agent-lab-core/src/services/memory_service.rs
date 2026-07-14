use chrono::Local;
use serde_json::Value;
use std::sync::Arc;

use crate::base::llm::AgentsLLM;
use crate::db::get_db_client;
use crate::error::AgentLabError;
use crate::services::ServiceError;
use crate::storage::{MemoryStore, Neo4jStore, OllamaEmbedder, PgStore};
use crate::tools::memory::base::{MemoryConfig, MemoryItem, MemoryWriteResult, RetrieveRequest};
use crate::tools::memory::engine::MemoryEngine;
use crate::tools::memory::fact_extractor::MemoryFactExtractor;
use crate::tools::memory::strategies::{
    EpisodicStrategy, PerceptualStrategy, SemanticStrategy, WorkingStrategy,
};

/// 记忆业务服务：面向应用层提供记忆 CRUD、搜索、统计、整合等能力。
pub struct MemoryService {
    #[allow(dead_code)]
    config: MemoryConfig,
    user_id: String,
    engine: MemoryEngine,
    fact_extractor: MemoryFactExtractor,
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
        let fact_extractor = MemoryFactExtractor::new(llm.clone());

        let mut strategies: Vec<Box<dyn crate::tools::memory::strategy::MemoryStrategy>> = Vec::new();
        if enable_working {
            strategies.push(Box::new(WorkingStrategy::new(config.clone())));
        }
        if enable_episodic {
            strategies.push(Box::new(EpisodicStrategy::new(llm.clone())));
        }
        if enable_semantic {
            strategies.push(Box::new(SemanticStrategy::new(llm.clone())));
        }
        if enable_perceptual {
            strategies.push(Box::new(PerceptualStrategy::new()));
        }

        let engine = MemoryEngine::new(store, config.clone(), strategies);

        Ok(Self {
            config,
            user_id,
            engine,
            fact_extractor,
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
        Ok(self
            .add_memory_with_result(content, memory_type, importance, metadata)
            .await?
            .memory_id)
    }

    /// 添加一条记忆并返回详细的写入结果（含冲突裁决信息）。
    pub async fn add_memory_with_result(
        &mut self,
        content: String,
        memory_type: String,
        importance: f32,
        metadata: Option<Value>,
    ) -> Result<MemoryWriteResult, AgentLabError> {
        let mut metadata = metadata.unwrap_or_else(|| self.build_memory_metadata());
        // 若调用方传入 metadata，仍补全 session_id / timestamp。
        if metadata.get("session_id").is_none() {
            metadata["session_id"] = Value::from(self.current_session_id.clone());
        }
        if metadata.get("timestamp").is_none() {
            metadata["timestamp"] = Value::from(Local::now().to_string());
        }

        let memory_item = MemoryItem::new(
            self.user_id.clone(),
            memory_type.clone(),
            content,
            importance as f64,
            metadata,
        );

        Ok(self.engine.add_with_result(memory_item).await)
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
            self.engine.memory_types()
        } else {
            memory_types.to_vec()
        };

        for memory_type in &types_to_search {
            let request = RetrieveRequest {
                query: query_owned.clone(),
                limit: Some(limit),
                user_id: Some(self.user_id.clone()),
                importance_threshold: Some(min_importance as f64),
                ..Default::default()
            };
            let results = self.engine.retrieve_by_type(memory_type, request).await;
            all_results.extend(results);
        }

        // 跨类型统一排序：使用各类型自身计算的 relevance_score，再乘以类型优先级。
        all_results.sort_by(|a, b| {
            let score_a = combined_search_score(a);
            let score_b = combined_search_score(b);
            score_b.total_cmp(&score_a)
        });
        all_results.truncate(limit);

        Ok(all_results)
    }

    /// 从对话上下文中提取事实，并内部决定记忆类型与重要性后批量存储。
    ///
    /// 这是给 `MemoryTool` 使用的“智能 add”入口：外部 AI 不再直接决定
    /// content / memory_type / importance，只提供原始上下文。
    ///
    /// 返回每条事实的写入结果，包含最终 memory_id 与冲突裁决动作。
    pub async fn add_memories_from_context(
        &mut self,
        context: &str,
    ) -> Result<Vec<MemoryWriteResult>, AgentLabError> {
        let facts = match self.fact_extractor.extract(context).await {
            Ok(facts) => facts,
            Err(e) => {
                tracing::warn!(
                    "[MemoryService] fact extraction failed: {}, fallback to store raw context",
                    e
                );
                vec![context.to_string()]
            }
        };

        // 统一构造 MemoryItem，再交给引擎做批量冲突裁决与存储。
        let mut items = Vec::with_capacity(facts.len());
        for fact in facts {
            let memory_type = route_memory_type(&fact);
            let importance = estimate_importance(&fact);
            let metadata = self.build_memory_metadata();
            items.push(MemoryItem::new(
                self.user_id.clone(),
                memory_type.to_string(),
                fact,
                importance as f64,
                metadata,
            ));
        }

        Ok(self.engine.add_batch(items).await)
    }

    fn build_memory_metadata(&self) -> serde_json::Value {
        let mut metadata = serde_json::json!({});
        metadata["session_id"] = Value::from(self.current_session_id.clone());
        metadata["timestamp"] = Value::from(Local::now().to_string());
        metadata
    }

    pub async fn forget_by_type(
        &mut self,
        memory_type: &str,
        strategy: &str,
        threshold: f32,
        max_age_days: i64,
    ) -> Result<usize, AgentLabError> {
        let count = self
            .engine
            .forget(memory_type, strategy, threshold as f64, max_age_days)
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
        &self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f32>,
        metadata: Option<Value>,
    ) -> Result<bool, AgentLabError> {
        let ok = self
            .engine
            .update(
                memory_id,
                content,
                importance.map(|v| v as f64),
                metadata.as_ref(),
            )
            .await?;
        Ok(ok)
    }

    pub async fn remove_memory(&self, memory_id: &str) -> Result<bool, AgentLabError> {
        let ok = self.engine.remove(memory_id).await?;
        Ok(ok)
    }

    pub async fn get_summary(
        &self,
        memory_type: &str,
        limit: usize,
    ) -> Result<String, AgentLabError> {
        let items = self
            .engine
            .list_by_type(memory_type, Some(&self.user_id), Some(limit as i64), false)
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
            .engine
            .count_by_type(memory_type, Some(&self.user_id))
            .await?;
        let avg_importance = self
            .engine
            .avg_importance_by_type(memory_type, Some(&self.user_id))
            .await?
            .unwrap_or(0.0);
        let time_span_days = self
            .engine
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
                let count = self.engine.clear_by_type(t).await?;
                Ok(count)
            }
            None => {
                let mut total = 0u64;
                for t in self.engine.memory_types() {
                    total += self.engine.clear_by_type(&t).await?;
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
                "{}. [{}] {} (ID: {}, 重要性: {})",
                i + 1,
                label,
                memory.content,
                memory.id,
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
        &mut self,
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

/// 跨类型搜索统一评分。
///
/// 优先使用各类型 `retrieve` 阶段写入 metadata 的 relevance_score；
/// 若不存在则回退到 importance。再乘以类型优先级做微调。
fn combined_search_score(item: &MemoryItem) -> f64 {
    let relevance = item
        .metadata
        .get("relevance_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(item.importance);
    let type_priority = match item.memory_type.as_str() {
        "semantic" => 1.05,
        "episodic" => 1.00,
        "working" => 0.95,
        "perceptual" => 0.90,
        _ => 0.90,
    };
    relevance * type_priority
}

/// 根据事实内容路由到合适的记忆类型。
fn route_memory_type(fact: &str) -> &'static str {
    let lower = fact.to_lowercase();
    if contains_time_info(&lower) {
        "episodic"
    } else if contains_personal_fact(&lower) {
        "semantic"
    } else if is_temporary(&lower) {
        "working"
    } else {
        "perceptual"
    }
}

fn contains_time_info(text: &str) -> bool {
    const TIME_KEYWORDS: &[&str] = &[
        "yesterday",
        "today",
        "tomorrow",
        "last week",
        "next week",
        "last month",
        "next month",
        "last year",
        "next year",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
        "昨天",
        "今天",
        "明天",
        "上周",
        "下周",
        "上个月",
        "下个月",
        "去年",
        "明年",
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
        "星期日",
        "周一",
        "周二",
        "周三",
        "周四",
        "周五",
        "周六",
        "周日",
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
        "点",
        "号",
        "日",
    ];
    TIME_KEYWORDS.iter().any(|kw| text.contains(kw))
}

fn contains_personal_fact(text: &str) -> bool {
    const PERSONAL_KEYWORDS: &[&str] = &[
        "name is",
        "i am",
        "i'm",
        "i like",
        "i love",
        "i hate",
        "i prefer",
        "my",
        "my name",
        "my job",
        "my work",
        "my hobby",
        "my family",
        "我叫",
        "我是",
        "我喜欢",
        "我讨厌",
        "我偏好",
        "我的",
        "我的名字",
        "我的工作",
    ];
    PERSONAL_KEYWORDS.iter().any(|kw| text.contains(kw))
}

fn is_temporary(text: &str) -> bool {
    const TEMP_KEYWORDS: &[&str] = &["now", "currently", "at the moment", "暂时", "目前", "当前"];
    TEMP_KEYWORDS.iter().any(|kw| text.contains(kw))
}

/// 启发式评估事实重要性。
fn estimate_importance(fact: &str) -> f32 {
    let lower = fact.to_lowercase();
    if contains_health_or_identity(&lower) {
        0.9
    } else if contains_intent_or_need(&lower) {
        0.75
    } else if contains_preference_or_plan(&lower) {
        0.6
    } else {
        0.45
    }
}

fn contains_health_or_identity(text: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "doctor",
        "appointment",
        "hospital",
        "medical",
        "symptom",
        "treatment",
        "cardiologist",
        "dentist",
        "name is",
        "i am",
        "i'm",
        "我叫",
        "我是",
        "医生",
        "医院",
        "预约",
        "症状",
        "治疗",
        "名字",
    ];
    KEYWORDS.iter().any(|kw| text.contains(kw))
}

fn contains_intent_or_need(text: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "want to",
        "need to",
        "plan to",
        "book",
        "schedule",
        "call",
        "appointment",
        "想要",
        "需要",
        "计划",
        "预约",
        "打电话",
    ];
    KEYWORDS.iter().any(|kw| text.contains(kw))
}

fn contains_preference_or_plan(text: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "like",
        "love",
        "hate",
        "prefer",
        "enjoy",
        "plan",
        "goal",
        "喜欢",
        "讨厌",
        "偏好",
        "计划",
        "目标",
    ];
    KEYWORDS.iter().any(|kw| text.contains(kw))
}
