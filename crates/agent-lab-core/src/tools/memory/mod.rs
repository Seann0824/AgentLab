pub mod base;
pub mod conflict_resolver;
pub mod engine;
pub mod extractor;
pub mod fact_extractor;
pub mod strategies;
pub mod strategy;

pub use base::{
    ConflictResolution, ExistingAction, ExistingMemoryDecision, Memory, MemoryConfig, MemoryItem,
    MemoryWriteAction, MemoryWriteResult, NewFactAction, RetrieveRequest,
};
pub use conflict_resolver::MemoryConflictResolver;
pub use engine::MemoryEngine;
pub use fact_extractor::MemoryFactExtractor;
pub use strategies::{EpisodicStrategy, PerceptualStrategy, SemanticStrategy, WorkingStrategy};
pub use strategy::{MemoryStrategy, StorageScope};

use std::collections::HashMap;
use std::sync::Arc;

use openai_api_rs::v1::types;
use tokio::sync::Mutex;

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::services::MemoryService;
use crate::tools::types::{Tool, ToolError};

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
        "记忆管理工具。当前仅向 AI 暴露 add(从对话上下文中智能提取并添加记忆), search(搜索所有类型记忆), update(更新已有记忆内容)。"
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let properties = HashMap::from([
            (
                "action".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some(
                        "要执行的操作: add(添加记忆), search(搜索记忆), update(更新记忆)".to_string(),
                    ),
                    enum_values: Some(vec![
                        "add".to_string(),
                        "search".to_string(),
                        "update".to_string(),
                    ]),
                    ..Default::default()
                }),
            ),
            (
                "context".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("add 时使用，待提取事实的对话上下文".to_string()),
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
                "memory_id".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("update 时必需，目标记忆ID".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "content".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("update 时使用，要更新的记忆内容".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "importance".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("update 时使用，重要性 0.0-1.0，默认 0.5".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "limit".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("search 时使用，返回条数上限，默认 5".to_string()),
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

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let action = args["action"].as_str().unwrap_or("");
        let mut inner = self.inner.lock().await;

        let result = match action {
            "add" => {
                let context = args["context"].as_str().unwrap_or("");
                if context.is_empty() {
                    return Err(ToolError::InvalidArgument("context 不能为空".to_string()));
                }
                inner
                    .memory_service
                    .add_memories_from_context(context)
                    .await
                    .map(|results| format_memory_add_results(&results))
            }
            "search" => {
                inner
                    .memory_service
                    .search_memories_agent(
                        args["query"].as_str(),
                        // 搜索不再由外部指定 memory_type，内部会搜索所有启用类型并统一排序。
                        None,
                        args["limit"].as_u64(),
                    )
                    .await
            }
            "summary" => {
                inner
                    .memory_service
                    .summary_agent(args["memory_type"].as_str(), args["limit"].as_u64())
                    .await
            }
            "stats" => {
                inner
                    .memory_service
                    .stats_agent(args["memory_type"].as_str())
                    .await
            }
            "update" => {
                inner
                    .memory_service
                    .update_memory_agent(
                        args["memory_id"].as_str(),
                        args["content"].as_str(),
                        args["importance"].as_f64().map(|v| v as f32),
                        args.get("metadata").cloned(),
                    )
                    .await
            }
            _ => {
                return Err(ToolError::InvalidArgument(format!(
                    "不支持的 action: {}。当前仅支持 add / search / update",
                    action
                )));
            }
        };

        result.map_err(|e| {
            let msg = e.to_string();
            if msg.starts_with("参数") || msg.contains("不能为空") {
                ToolError::InvalidArgument(msg)
            } else {
                ToolError::external("MemoryService", msg)
            }
        })
    }
}

fn format_memory_add_results(results: &[MemoryWriteResult]) -> String {
    if results.is_empty() {
        return "未提取到可存储的事实".to_string();
    }

    let mut lines = vec![format!("已处理 {} 条记忆：", results.len())];
    for result in results {
        let label = match result.action {
            MemoryWriteAction::Added => "新增",
            MemoryWriteAction::SkippedDuplicate => "跳过",
            MemoryWriteAction::Merged => "合并",
            MemoryWriteAction::InvalidatedOthers => "更新",
        };
        let detail = if result.invalidated_ids.is_empty() {
            String::new()
        } else {
            format!("（已失效旧记忆: {}）", result.invalidated_ids.join(", "))
        };
        lines.push(format!(
            "- [{}] {} (ID: {}){}",
            label, result.fact, result.memory_id, detail
        ));
    }

    lines.join("\n")
}
