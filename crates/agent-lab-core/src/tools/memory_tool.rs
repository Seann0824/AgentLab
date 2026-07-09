use std::collections::HashMap;

use openai_api_rs::v1::types;
use tokio::sync::Mutex;

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::services::MemoryService;
use crate::tools::types::Tool;

/// 记忆管理 Tool：面向 LLM 提供记忆增删改查、整合、遗忘能力。
pub struct MemoryTool {
    inner: Mutex<MemoryToolInner>,
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
            inner: Mutex::new(MemoryToolInner { memory_service }),
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
                let id = inner
                    .memory_service
                    .add_memory(content, memory_type, importance.unwrap_or(0.5), None)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(format!("记忆已添加 （ID: {}）", id))
            }
            "search" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                if query.is_empty() {
                    return Err("query 不能为空".into());
                }
                let limit = args["limit"].as_u64().map(|v| v as usize).unwrap_or(5);
                let memory_type = args["memory_type"].as_str().map(|v| v.to_string());
                let memory_types = memory_type.map(|t| vec![t]).unwrap_or_default();

                let results = inner
                    .memory_service
                    .search_memories(&query, limit, &memory_types, 0.1)
                    .await
                    .map_err(|e| e.to_string())?;

                if results.is_empty() {
                    return Ok(format!("未找到与 {} 相关的记忆", query));
                }

                let type_label_map = HashMap::from([
                    ("working", "工作记忆"),
                    ("episodic", "情景记忆"),
                    ("semantic", "语义记忆"),
                    ("perceptual", "感知记忆"),
                ]);

                let mut formatted = vec![format!("找到 {} 条相关记忆", results.len())];
                for (i, memory) in results.iter().enumerate() {
                    let label = type_label_map
                        .get(memory.memory_type.as_str())
                        .unwrap_or(&"未知类型");
                    let preview = if memory.content.len() > 80 {
                        format!("{} ...", memory.content.chars().take(80).collect::<String>())
                    } else {
                        memory.content.clone()
                    };
                    formatted.push(format!(
                        "{}. [{}] {} (重要性: {})",
                        i + 1,
                        label,
                        preview,
                        memory.importance
                    ));
                }
                Ok(formatted.join("\n"))
            }
            "summary" => {
                let memory_type = args["memory_type"].as_str().unwrap_or("working");
                let limit = args["limit"].as_u64().map(|v| v as usize).unwrap_or(5);
                inner
                    .memory_service
                    .get_summary(memory_type, limit)
                    .await
                    .map_err(|e| e.to_string())
            }
            "stats" => {
                let memory_type = args["memory_type"].as_str().unwrap_or("working");
                inner
                    .memory_service
                    .get_stats(memory_type)
                    .await
                    .map_err(|e| e.to_string())
            }
            "update" => {
                let memory_id = args["memory_id"].as_str().unwrap_or("").to_string();
                if memory_id.is_empty() {
                    return Err("memory_id 不能为空".into());
                }
                let content = args["content"].as_str().map(|v| v.to_string());
                let importance = args["importance"].as_f64().map(|v| v as f32);
                let metadata = args.get("metadata").cloned();
                let ok = inner
                    .memory_service
                    .update_memory(&memory_id, content.as_deref(), importance, metadata)
                    .await
                    .map_err(|e| e.to_string())?;
                if ok {
                    Ok(format!("记忆 {} 更新成功", memory_id))
                } else {
                    Ok(format!("未找到记忆 {}", memory_id))
                }
            }
            "remove" => {
                let memory_id = args["memory_id"].as_str().unwrap_or("").to_string();
                if memory_id.is_empty() {
                    return Err("memory_id 不能为空".into());
                }
                let ok = inner
                    .memory_service
                    .remove_memory(&memory_id)
                    .await
                    .map_err(|e| e.to_string())?;
                if ok {
                    Ok(format!("记忆 {} 已删除", memory_id))
                } else {
                    Ok(format!("未找到记忆 {}", memory_id))
                }
            }
            "forget" => {
                let memory_type = args["memory_type"].as_str().unwrap_or("working");
                let strategy = args["strategy"].as_str().unwrap_or("importance_based");
                let threshold = args["threshold"].as_f64().map(|v| v as f32).unwrap_or(0.1);
                let max_age_days = args["max_age_days"].as_u64().map(|v| v as i64).unwrap_or(30);
                let count = inner
                    .memory_service
                    .forget_by_type(memory_type, strategy, threshold, max_age_days)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "已遗忘 {} 条 {} 记忆（策略: {}）",
                    count, memory_type, strategy
                ))
            }
            "consolidate" => {
                let from_type = args["from_type"].as_str().unwrap_or("working").to_string();
                let to_type = args["to_type"].as_str().unwrap_or("episodic").to_string();
                let importance_threshold = args["importance_threshold"]
                    .as_f64()
                    .map(|v| v as f32)
                    .unwrap_or(0.7);
                let count = inner
                    .memory_service
                    .consolidate_memories(&from_type, &to_type, importance_threshold)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "已整合 {} 条记忆为长期记忆（{} → {}，阈值={}）",
                    count, from_type, to_type, importance_threshold
                ))
            }
            "clear_all" => {
                let memory_type = args["memory_type"].as_str();
                let count = inner
                    .memory_service
                    .clear_all(memory_type)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(format!("已清空 {} 条记忆", count))
            }
            _ => Err(format!("不支持的 action: {}", action)),
        }
    }
}
