use std::collections::HashMap;
use std::sync::Arc;

use openai_api_rs::v1::types;
use tokio::sync::Mutex;

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::services::MemoryService;
use crate::tools::types::Tool;

/// 记忆管理 Tool：面向 LLM 提供记忆增删改查、整合、遗忘能力。
#[derive(Clone)]
pub struct MemoryTool {
    inner: Arc<Mutex<MemoryToolInner>>,
}

struct MemoryToolInner {
    memory_service: MemoryService,
}

impl MemoryTool {
    pub async fn new(
        llm: AgentsLLM,
        database_url: impl Into<String>,
        neo4j_uri: impl Into<String>,
        neo4j_user: impl Into<String>,
        neo4j_password: impl Into<String>,
    ) -> Result<Self, AgentLabError> {
        let memory_service = MemoryService::new(
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
        .await?;

        Ok(Self {
            inner: Arc::new(Mutex::new(MemoryToolInner { memory_service })),
        })
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
        let mut inner = self.inner.lock().await;

        match action {
            "add" => inner
                .memory_service
                .add_memory_agent(
                    args["content"].as_str(),
                    args["memory_type"].as_str(),
                    args["importance"].as_f64().map(|v| v as f32),
                )
                .await
                .map_err(|e| e.to_string()),
            "search" => inner
                .memory_service
                .search_memories_agent(
                    args["query"].as_str(),
                    args["memory_type"].as_str(),
                    args["limit"].as_u64(),
                )
                .await
                .map_err(|e| e.to_string()),
            "summary" => inner
                .memory_service
                .summary_agent(args["memory_type"].as_str(), args["limit"].as_u64())
                .await
                .map_err(|e| e.to_string()),
            "stats" => inner
                .memory_service
                .stats_agent(args["memory_type"].as_str())
                .await
                .map_err(|e| e.to_string()),
            "update" => inner
                .memory_service
                .update_memory_agent(
                    args["memory_id"].as_str(),
                    args["content"].as_str(),
                    args["importance"].as_f64().map(|v| v as f32),
                    args.get("metadata").cloned(),
                )
                .await
                .map_err(|e| e.to_string()),
            "remove" => inner
                .memory_service
                .remove_memory_agent(args["memory_id"].as_str())
                .await
                .map_err(|e| e.to_string()),
            "forget" => inner
                .memory_service
                .forget_by_type_agent(
                    args["memory_type"].as_str(),
                    args["strategy"].as_str(),
                    args["threshold"].as_f64().map(|v| v as f32),
                    args["max_age_days"].as_u64(),
                )
                .await
                .map_err(|e| e.to_string()),
            "consolidate" => inner
                .memory_service
                .consolidate_memories_agent(
                    args["from_type"].as_str(),
                    args["to_type"].as_str(),
                    args["importance_threshold"].as_f64().map(|v| v as f32),
                )
                .await
                .map_err(|e| e.to_string()),
            "clear_all" => inner
                .memory_service
                .clear_all_agent(args["memory_type"].as_str())
                .await
                .map_err(|e| e.to_string()),
            _ => Err(format!("不支持的 action: {}", action)),
        }
    }
}
