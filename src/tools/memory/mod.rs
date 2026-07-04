use std::{collections::HashMap, sync::Mutex};
use chrono::Local;
use openai_api_rs::v1::types;
use serde_json::Value;

use crate::tools::types::Tool;
mod base;
mod embedder;
mod working_memory;
mod episodic_memory;
mod semantic_memory;
mod perceptual_memory;
use base::{Memory, MemoryItem, MemoryConfig, MemoryRetriever, MemoryStore};
use working_memory::WorkingMemory;
use episodic_memory::EpisodicMemory;
use semantic_memory::SemanticMemory;
use perceptual_memory::PerceptualMemory;

pub struct MemoryTool {
    inner: Mutex<MemoryToolInner>,
}

struct MemoryToolInner {
    current_session_id: Option<String>,
    memory_manager: MemoryManager,
}

impl MemoryTool {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MemoryToolInner {
                current_session_id: None,
                memory_manager: MemoryManager::new(None, None, None, None, None, None),
            }),
        }
    }

    fn add_memory(
        &self,
        content: String,
        memory_type: String,
        importance: Option<f32>,
        _file_path: Option<String>,
        _modality: Option<String>,
        metadata: impl Into<Option<Value>>,
    ) -> String {
        let importance = importance.unwrap_or(0.5f32);
        let mut inner = self.inner.lock().unwrap();

        // 没有则分配会话id
        if inner.current_session_id.is_none() {
            inner.current_session_id = Some(format!(
                "session_{}",
                Local::now().format("%Y%m%d_%H%M%S"),
            ));
        }

        let mut metadata: Value = metadata.into().unwrap_or_else(|| serde_json::json!({}));

        // 添加会话信息到元数据
        metadata["session_id"] = Value::from(inner.current_session_id.clone());
        metadata["timestamp"] = Value::from(Local::now().to_string());

        let memory_id = inner.memory_manager.add_memory(
            content,
            memory_type,
            importance,
            metadata,
            false,
        );

        match memory_id {
            Ok(id) => format!("记忆已添加 （ID: {}）", id),
            Err(e) => format!("记忆添加失败: {}", e),
        }
    }

    fn search_memory(
        &self,
        query: String,
        limit: Option<usize>,
        memory_types: Option<Vec<String>>,
        memory_type: Option<String>,
        min_importance: Option<f32>
    ) -> String {
        let min_importance = min_importance.unwrap_or(0.1f32);
        let mut memory_types = memory_types.unwrap_or_default();
        let limit = limit.unwrap_or(5usize);

        if memory_type.is_some() && memory_types.is_empty() {
            memory_types.push(memory_type.unwrap().clone());
        }

        let mut inner = self.inner.lock().unwrap();
        let results = inner.memory_manager.retrieve_memories(
            &query,
            limit,
            &memory_types,
            min_importance,
        );

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
                        ("perceptual", "感知记忆")
                    ]);
                    let memory_type_label = type_label_map.get(memory.memory_type.as_str()).unwrap_or(&"未知类型");
                    let content_preview = if memory.content.len() > 80usize {
                        format!("{} ...", memory.content.chars().take(80).collect::<String>())
                    } else {
                        memory.content.clone()
                    };

                    formatted_results.push(
                        format!("{}. [{}] {} (重要性: {})", i + 1, memory_type_label, content_preview, memory.importance)
                    );
                }

               formatted_results.join("\n")
            },
            Err(msg) => format!("搜索记忆失败：{}", msg)
        }
    }

    fn fortget(&self, strategy: String, threshold: Option<f32>, max_age_days: Option<usize>) -> String {
        let threshold = threshold.unwrap_or(0.1);
        let max_age_days = max_age_days.unwrap_or(30);
        let inner = self.inner.lock().unwrap();
        match inner.memory_manager.forget(&strategy, threshold, max_age_days) {
            Ok(count) => format!("已遗忘 {} 条记忆（策略: {}）", count, strategy),
            Err(msg) => format!("遗忘记忆失败: {}", msg),
        }
    }

    // 短期记忆提升为长期记忆
    fn consolidate(&self, from_type: Option<String>, to_type: Option<String>, importance_threshold: Option<f32>) -> String {
        let from_type = from_type.unwrap_or("working".to_string());
        let to_type = to_type.unwrap_or("episodic".to_string());
        let importance_threshold = importance_threshold.unwrap_or(0.7);
        let mut inner = self.inner.lock().unwrap();
        match inner.memory_manager.consolidate_memories(&from_type, &to_type, importance_threshold) {
            Ok(count) => format!("已整合 {} 条记忆为长期记忆（{} → {}，阈值={}）", count, from_type, to_type, importance_threshold),
            Err(msg) => format!("整合记忆失败: {}", msg)
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryTool {
    fn name(&self) ->  &str {
        "memory"
    }

    fn description(&self) ->  &str {
        "记忆管理工具。当需要保存用户的关键信息（如偏好、身份、重要事实）以便后续回忆时，使用 action='add'；当需要根据当前问题查找历史记忆时，使用 action='search'。"
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let properties = HashMap::from([
            (
                "action".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("要执行的操作: add（添加记忆）或 search（搜索记忆）".to_string()),
                    enum_values: Some(vec![
                        "add".to_string(),
                        "search".to_string(),
                    ]),
                    ..Default::default()
                }),
            ),
            (
                "content".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("add 操作时必填，要保存的记忆内容".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "memory_type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("记忆类型，默认 working".to_string()),
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
                "query".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("search 操作时必填，搜索关键词".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "importance".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Number),
                    description: Some("add 时使用，重要性 0.0-1.0，默认 0.5".to_string()),
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

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().unwrap_or("");
        match action {
            "add" => {
                let content = args["content"].as_str().unwrap_or("").to_string();
                if content.is_empty() {
                    return Err("content 不能为空".into());
                }
                let memory_type = args["memory_type"].as_str().unwrap_or("working").to_string();
                let importance = args["importance"].as_f64().map(|v| v as f32);
                Ok(self.add_memory(content, memory_type, importance, None, None, None))
            }
            "search" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                if query.is_empty() {
                    return Err("query 不能为空".into());
                }
                let limit = args["limit"].as_u64().map(|v| v as usize);
                let memory_type = args["memory_type"].as_str().map(|v| v.to_string());
                let memory_types = memory_type.map(|t| vec![t]);
                Ok(self.search_memory(query, limit, memory_types, None, None))
            }
            _ => Err(format!("不支持的 action: {}", action)),
        }
    }
}

pub struct MemoryManager {
    config: MemoryConfig,
    user_id: String,
    store: MemoryStore,
    retriever: MemoryRetriever,
    memory_types: HashMap<String, Box<dyn Memory>>
}

impl MemoryManager {
    pub fn new(
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

        let store = MemoryStore::new(config.clone());
        let retriever = MemoryRetriever::new(store.clone(), config.clone());

        let mut memory_types: HashMap<String, Box<dyn Memory>> = HashMap::new();

        if enable_working {
            memory_types.insert("working".into(), Box::new(WorkingMemory::new(config.clone(), store.clone())));
        }
        if enable_episodic {
            memory_types.insert("episodic".into(), Box::new(EpisodicMemory::new(config.clone(), store.clone())));
        }
        if enable_semantic {
            memory_types.insert("semantic".into(), Box::new(SemanticMemory::new(config.clone(), store.clone())));
        }
        if enable_perceptual {
            memory_types.insert("perceptual".into(), Box::new(PerceptualMemory::new(config.clone(), store.clone())));
        }

        Self {
            config,
            user_id,
            store,
            retriever,
            memory_types,
        }
    }

    pub fn add_memory(
        &mut self,
        content: String,
        memory_type: String,
        importance: f32,
        _metadata: Value,
        auto_classify: bool,
    ) -> Result<String, String> {
        let memory_id = format!("mem_{}", Local::now().timestamp_millis());
        let memory_item = MemoryItem {
            id: memory_id.clone(),
            memory_type: memory_type.clone(),
            content,
            timestamp: Local::now().timestamp(),
            importance: importance as f64,
        };

        let target_type = if auto_classify {
            // 简单自动分类：后续可扩展为根据内容选择最合适的记忆类型
            memory_type
        } else {
            memory_type
        };

        let Some(memory_store) = self.memory_types.get_mut(&target_type) else {
            return Err(format!("记忆类型 {} 不存在", target_type));
        };

        memory_store.add(memory_item);
        Ok(memory_id)
    }

    pub fn retrieve_memories(
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
            let results = memory_store.retrieve(&query_owned, Some(limit), None);
            all_results.extend(results);
        }

        let min_importance = min_importance as f64;
        all_results.retain(|m| m.importance >= min_importance);
        all_results.sort_by(|a, b| b.importance.total_cmp(&a.importance));
        all_results.truncate(limit);

        Ok(all_results)
    }

    pub fn forget(&self, _strategy: &String, _threshold: f32, _max_age_days: usize) -> Result<usize, String> {
        Ok(0)
    }

    pub fn consolidate_memories(&mut self, _from_type: &String, _to_type: &String, _importance_threshold: f32) -> Result<usize, String> {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_manager_add_and_retrieve() {
        let mut manager = MemoryManager::new(None, None, Some(true), Some(false), Some(false), Some(false));
        let id = manager.add_memory(
            "我最喜欢的颜色是蓝色".to_string(),
            "working".to_string(),
            0.8,
            serde_json::json!({}),
            false,
        ).unwrap();
        assert!(!id.is_empty());

        let results = manager.retrieve_memories(
            "喜欢的颜色",
            5,
            &vec!["working".to_string()],
            0.0,
        ).unwrap();
        assert!(!results.is_empty(), "应该能召回工作记忆");
        assert!(results.iter().any(|m| m.content.contains("蓝色")));
    }

    #[test]
    fn test_memory_tool_add_and_search() {
        let tool = MemoryTool::new();
        let add_result = tool.add_memory(
            "我的职业是工程师".to_string(),
            "working".to_string(),
            Some(0.9),
            None,
            None,
            None,
        );
        assert!(add_result.contains("记忆已添加"));

        let search_result = tool.search_memory(
            "职业".to_string(),
            Some(5),
            Some(vec!["working".to_string()]),
            None,
            None,
        );
        assert!(search_result.contains("工程师"), "搜索结果应包含工程师: {}", search_result);
    }
}
