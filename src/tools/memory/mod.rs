use std::{collections::HashMap, hash::Hash};
use chrono::{Date, DateTime, Local, TimeZone};
use openai_api_rs::v1::{file, types};
use serde_json::{Value, json};

use crate::tools::types::{Tool};


struct Memory {
    current_session_id: Option<String>,
    memory_manager: MemoryManager,
}

impl Memory {
    
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
}

#[async_trait::async_trait]
impl Tool for Memory {
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

struct MemoryItem {
    memory_type: String,
    content: String,
}

pub struct MemoryManager {

}

impl MemoryManager {
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
}