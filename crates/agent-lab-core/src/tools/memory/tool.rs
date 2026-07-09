use chrono::Local;
use openai_api_rs::v1::types;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::base::llm::AgentsLLM;
use crate::tools::memory::manager::MemoryManager;
use crate::tools::types::Tool;

pub struct MemoryTool {
    inner: Mutex<MemoryToolInner>,
}

struct MemoryToolInner {
    current_session_id: Option<String>,
    memory_manager: MemoryManager,
}

impl MemoryTool {
    pub async fn new(
        llm: AgentsLLM,
        database_url: impl Into<String>,
        neo4j_uri: impl Into<String>,
        neo4j_user: impl Into<String>,
        neo4j_password: impl Into<String>,
    ) -> Self {
        Self {
            inner: Mutex::new(MemoryToolInner {
                current_session_id: None,
                memory_manager: MemoryManager::new(
                    None,
                    None,
                    llm,
                    database_url,
                    neo4j_uri,
                    neo4j_user,
                    neo4j_password,
                    None,
                    None,
                    None,
                    None,
                )
                .await,
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
                Some(format!("session_{}", Local::now().format("%Y%m%d_%H%M%S")));
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
                        format!("{} ...", memory.content.chars().take(80).collect::<String>())
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
        let inner = self.inner.lock().await;
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
