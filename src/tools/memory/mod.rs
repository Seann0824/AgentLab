use std::{collections::HashMap, hash::Hash};
use chrono::{Date, DateTime, Local, TimeZone};
use openai_api_rs::v1::{file, types};
use serde_json::{Value, json};

use crate::{base::config::Config, tools::types::Tool};
mod base;
mod working_memory;
mod episodic_memory;
mod semantic_memory;
mod perceptual_memory;
use base::{Memory, MemoryItem, MemoryConfig, MemoryRetriever, MmeoryStore};
use working_memory::WorkingMemory;
use episodic_memory::EpisodicMemory;
use semantic_memory::SemanticMemory;
use perceptual_memory::PerceptualMemory;

struct MemoryTool {
    current_session_id: Option<String>,
    memory_manager: MemoryManager,
}

impl MemoryTool {
    
    fn add_memory(
        &mut self, 
        content: String,
        memory_type: String,
        importance: Option<f32>,
        file_path: Option<String>,
        modality: Option<String>,
        metadata: impl Into<Option<Value>>,
    ) -> String {
        let importance = importance.unwrap_or(0.5f32);

        // 没有则分配会话id
        if self.current_session_id.is_none() {
            self.current_session_id = Some(format!(
                "session_{}",
                Local::now().format("%Y%m%d_%H%M%S"),
            ));
        }

        let mut metadata: Value = metadata.into().and_then(|_| Some(Value::default())).unwrap();

        // 感知记忆文件支持
        if memory_type == "percceptual" && let Some(file_path) = file_path {
            let inferred = modality.and_then(|_| self.infer_modality(&file_path)).unwrap();
            metadata["modality"] = Value::from(inferred);
            metadata["raw_data"] = Value::from(file_path);
        }

        // 添加会话信息到元数据
        metadata["session_id"] = Value::from(self.current_session_id.clone());
        metadata["timestamp"] = Value::from(Local::now().to_string());

        let memory_id = self.memory_manager.add_memory(
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

    fn infer_modality(&self, file_path: &str) -> Option<String> {
        Some("".to_string())
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

        let results = self.memory_manager.retrieve_memories(
            &query,
            limit,
            &memory_types,
            min_importance,
        );
        
        match results {
            Ok(results) => {
                if results.is_empty() {
                    return format!("未找到与 {query} 相关的记忆");
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
                    let memory_type_label = type_label_map.get(memory.memory_type.as_str()).unwrap();
                    let content_preview = if memory.content.len() > 80usize {
                        format!("{} ...", memory.content.chars().take(80).collect::<String>())
                    } else {
                        memory.content.clone()
                    };

                    formatted_results.push(
                        format!("{i}. [{}] {content_preview} (重要性: {})", memory_type_label, min_importance)
                    );
                }

               formatted_results.join("\n")
            },
            Err(msg) => format!("搜索记忆失败：{}", msg)
        }
    }

    fn fortget(&mut self, strategy: String, threshold: Option<f32>, max_age_days: Option<usize>) -> String {
        let threshold = threshold.unwrap_or(0.1);
        let max_age_days = max_age_days.unwrap_or(30);
        match self.memory_manager.forget(&strategy, threshold, max_age_days) {
            Ok(count) => format!("已遗忘 {count} 条记忆（策略: {strategy}）"),
            Err(msg) => format!("遗忘记忆失败: {}", msg),
        }
    }

    // 短期记忆提升为长期记忆
    fn consolidate(&mut self, from_type: Option<String>, to_type: Option<String>, importance_threshold: Option<f32>) -> String {
        let from_type = from_type.unwrap_or("working".to_string());
        let to_type = to_type.unwrap_or("episodic".to_string());
        let importance_threshold = importance_threshold.unwrap_or(0.7);
        match self.memory_manager.consolidate_memories(&from_type, &to_type, importance_threshold) {
            Ok(count) => format!("已整合 {count} 条记忆为长期记忆（{from_type} → {to_type}，阈值={importance_threshold}）"),
            Err(msg) => format!("整合记忆失败: {}", msg)
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryTool {
    fn name(&self) ->  &str {
        todo!()
    }

    fn description(&self) ->  &str {
        todo!()
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let properties = HashMap::from([
            (
                "action".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Array),
                    description: Some("搜索关键词".to_string()),
                    enum_values: Some(vec![
                        "add".to_string(),
                        "search".to_string(),
                        "summary".to_string()
                    ]),
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
    // 1. 获取当前 action

    // 2. 不同 action 有不同的处理逻辑
    todo!()
    }
}




pub struct MemoryManager {
    config: MemoryConfig,
    user_id: String,
    store: MmeoryStore,
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

        let store = MmeoryStore::new(config.clone());
        let retriever = MemoryRetriever::new(store.clone(), config.clone());

        let mut memory_types: HashMap<String, Box<dyn Memory>> = HashMap::new();

        if enable_working {
            memory_types.insert("working".into(), Box::new(WorkingMemory::new(config.clone(), store.clone())));
        }
        if enable_episodic {
            memory_types.insert("working".into(), Box::new(EpisodicMemory::new(config.clone(), store.clone())));
        }
        if enable_semantic {
            memory_types.insert("working".into(), Box::new(SemanticMemory::new(config.clone(), store.clone())));
        }
        if enable_perceptual {
            memory_types.insert("working".into(), Box::new(PerceptualMemory::new(config.clone(), store.clone())));
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
        metadata: Value,
        auto_classify: bool,
    ) -> Result<String, String> {
        let memory_id = Local::now().to_string();
        Ok(memory_id)
    }

    pub fn retrieve_memories(
        &self,
        query: &str,
        limit: usize,
        memory_types: &Vec<String>,
        min_importance: f32,
    ) -> Result<Vec<MemoryItem>, String> {
        Ok(vec![])
    }

    pub fn forget(&self, strategy: &String, threshold: f32, max_age_days: usize) -> Result<usize, String> {
        Ok(2)
    }

    pub fn consolidate_memories(&mut self, from_type: &String, to_type: &String, importance_threshold: f32) -> Result<usize, String> {
        Ok(2)
    }
}

