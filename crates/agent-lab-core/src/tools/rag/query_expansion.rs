use std::collections::HashMap;

use openai_api_rs::v1::types;
use serde_json::Value;

use crate::agent::tool_agent::ToolAgent;
use crate::base::llm::AgentsLLM;
use crate::tools::ToolManager;
use crate::tools::types::Tool;

/// MQE（Multi-Query Expansion）子 agent：把用户的一个问题扩展成多个语义等价的查询句。
///
/// 参考 memory 的 `EntityExtractorAgent` 实现：基于 `ToolAgent<T>`，
/// 只注册一个约束输出格式的工具，由 LLM 按 schema 生成结构化结果。
pub struct QueryExpansionAgent {
    inner: ToolAgent<ExpansionResult>,
}

#[derive(serde::Deserialize)]
pub struct ExpansionResult {
    pub queries: Vec<String>,
}

impl QueryExpansionAgent {
    pub fn from_env() -> Self {
        Self::new(AgentsLLM::from_env())
    }

    pub fn new(llm: AgentsLLM) -> Self {
        let system_prompt = r#"
        你是一名查询扩展专家。用户会给你一个问题，请你生成 3~5 个语义等价但表述不同的查询句。

        目标：
        - 用不同的关键词、句式、同义词改写原问题。
        - 保持每个查询句都能独立表达原问题的检索意图。
        - 如果原问题包含专业术语，可以补充更口语化或更正式的变体。

        必须调用 expand_queries 工具输出结果。
        "#;

        let mut tool_manager = ToolManager::new();
        tool_manager.register_tool(Box::new(ExpandQueriesTool));

        let inner = ToolAgent::new("query_expander", llm, system_prompt, tool_manager);

        Self { inner }
    }

    /// 把 `query` 扩展成多个等价查询句。
    pub async fn expand(&mut self, query: &str) -> Result<Vec<String>, String> {
        let result = self.inner.run(query).await?;
        Ok(result.queries)
    }
}

/// 子 agent 唯一拥有的工具：只提供 schema，实际不执行任何操作。
///
/// 真正的“执行”发生在 LLM 侧——模型根据 schema 生成参数，
/// `ToolAgent` 再把返回值反序列化为结构化结果。
struct ExpandQueriesTool;

#[async_trait::async_trait]
impl Tool for ExpandQueriesTool {
    fn name(&self) -> &str {
        "expand_queries"
    }

    fn description(&self) -> &str {
        "把用户查询扩展成多个语义等价的查询句"
    }

    fn parameters_schema(&self) -> types::FunctionParameters {
        let query_item = types::JSONSchemaDefine {
            schema_type: Some(types::JSONSchemaType::String),
            description: Some("一个语义等价的查询句".to_string()),
            ..Default::default()
        };

        types::FunctionParameters {
            schema_type: types::JSONSchemaType::Object,
            properties: Some(HashMap::from([(
                "queries".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Array),
                    description: Some("扩展后的查询句列表，3~5 条".to_string()),
                    items: Some(Box::new(query_item)),
                    ..Default::default()
                }),
            )])),
            required: Some(vec!["queries".to_string()]),
        }
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        serde_json::to_string(&args)
            .map_err(|e| format!("[ExpandQueriesTool] serialize args failed: {}", e))
    }
}
