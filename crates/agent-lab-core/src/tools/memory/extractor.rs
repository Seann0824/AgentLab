use std::collections::HashMap;

use openai_api_rs::v1::types;
use serde_json::Value;

use crate::agent::tool_agent::ToolAgent;
use crate::base::llm::AgentsLLM;
use crate::tools::ToolManager;
use crate::storage::neo4j::{EntityInput, RelationInput};
use crate::tools::types::Tool;

/// 内部子 agent：专门负责从记忆内容里抽取实体和关系。
///
/// 它基于通用 `ToolAgent<T>` 实现，只注册一个 `ExtractEntitiesRelationsTool`，
/// 通过 OpenAI 的 function calling 让模型按固定 schema 输出结构化结果。
pub struct EntityExtractorAgent {
    inner: ToolAgent<ExtractionResult>,
}

#[derive(serde::Deserialize)]
struct ExtractionResult {
    entities: Vec<EntityInput>,
    relations: Vec<RelationInput>,
}

impl EntityExtractorAgent {
    pub fn new(llm: AgentsLLM) -> Self {
        let system_prompt = r#"
        你是一名实体与关系抽取专家。用户会给你一段记忆内容，请你：
        1. 识别内容中的关键实体（人、地点、组织、事件、概念等）。
        2. 判断实体之间的关系。
        3. 必须调用 extract_entities_relations 工具输出结果。

        注意：
        - 只需要输出实体的 name 和 type，不需要 id。
        - relation 请用实体的 name 和 type 来标识起点和终点。
        - 如果内容中没有明显实体，返回空数组。
        "#;

        let mut tool_manager = ToolManager::new();
        tool_manager.register_tool(Box::new(ExtractEntitiesRelationsTool));

        let inner = ToolAgent::new("entity_extractor", llm, system_prompt, tool_manager);

        Self { inner }
    }

    /// 从 content 中抽取实体和关系。
    ///
    /// 返回的 `Vec` 可能为空（模型认为没有实体时），调用方可以据此决定是否写入 Neo4j。
    pub async fn extract(
        &mut self,
        content: &str,
    ) -> Result<(Vec<EntityInput>, Vec<RelationInput>), String> {
        let result = self.inner.run(content).await?;
        Ok((result.entities, result.relations))
    }
}

/// 子 agent 唯一拥有的工具：只提供 schema，实际不执行任何操作。
///
/// 真正的“执行”发生在 LLM 侧——模型根据 schema 生成参数，业务层再解析这些参数。
struct ExtractEntitiesRelationsTool;

#[async_trait::async_trait]
impl Tool for ExtractEntitiesRelationsTool {
    fn name(&self) -> &str {
        "extract_entities_relations"
    }

    fn description(&self) -> &str {
        "从记忆内容中抽取实体和关系"
    }

    fn parameters_schema(&self) -> types::FunctionParameters {
        extraction_schema()
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        // 该工具仅用于约束 LLM 的输出格式，执行时直接把参数原样返回，
        // 由 ToolAgent 把返回值反序列化为结构化结果。
        serde_json::to_string(&args)
            .map_err(|e| format!("[ExtractEntitiesRelationsTool] serialize args failed: {}", e))
    }
}

fn extraction_schema() -> types::FunctionParameters {
    let entity_item = types::JSONSchemaDefine {
        schema_type: Some(types::JSONSchemaType::Object),
        properties: Some(HashMap::from([
            (
                "name".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("实体名称".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("实体类型，例如 PERSON、LOCATION、ORG、EVENT".to_string()),
                    ..Default::default()
                }),
            ),
        ])),
        required: Some(vec!["name".to_string(), "type".to_string()]),
        ..Default::default()
    };

    let relation_item = types::JSONSchemaDefine {
        schema_type: Some(types::JSONSchemaType::Object),
        properties: Some(HashMap::from([
            (
                "from_name".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("关系起点实体的 name".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "from_type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("关系起点实体的 type".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "to_name".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("关系终点实体的 name".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "to_type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("关系终点实体的 type".to_string()),
                    ..Default::default()
                }),
            ),
            (
                "relation_type".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("关系类型，例如 去过、同事、属于".to_string()),
                    ..Default::default()
                }),
            ),
        ])),
        required: Some(vec![
            "from_name".to_string(),
            "from_type".to_string(),
            "to_name".to_string(),
            "to_type".to_string(),
            "relation_type".to_string(),
        ]),
        ..Default::default()
    };

    types::FunctionParameters {
        schema_type: types::JSONSchemaType::Object,
        properties: Some(HashMap::from([
            (
                "entities".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Array),
                    description: Some("抽取出的实体列表".to_string()),
                    items: Some(Box::new(entity_item)),
                    ..Default::default()
                }),
            ),
            (
                "relations".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Array),
                    description: Some("实体之间的关系列表".to_string()),
                    items: Some(Box::new(relation_item)),
                    ..Default::default()
                }),
            ),
        ])),
        required: Some(vec!["entities".to_string(), "relations".to_string()]),
    }
}
