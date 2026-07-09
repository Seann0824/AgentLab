use std::collections::HashMap;

use openai_api_rs::v1::types;
use serde_json::Value;

use crate::agent::tool_agent::ToolAgent;
use crate::base::llm::AgentsLLM;
use crate::tools::ToolManager;
use crate::tools::types::Tool;

/// HyDE（Hypothetical Document Embeddings）子 agent：让 LLM 先根据问题生成一段假设答案，
/// 再用这段假设答案的 embedding 去检索真实文档，从而缩小「问题」与「答案」之间的语义鸿沟。
///
/// 参考 memory 的 `EntityExtractorAgent` 实现：基于 `ToolAgent<T>`，
/// 只注册一个约束输出格式的工具，由 LLM 按 schema 输出结构化结果。
pub struct HydeAgent {
    inner: ToolAgent<HydeResult>,
}

#[derive(serde::Deserialize)]
pub struct HydeResult {
    pub hypothetical_document: String,
}

impl HydeAgent {
    pub fn new(llm: AgentsLLM) -> Self {
        let system_prompt = r#"
        你是一名假设文档生成专家。用户会给你一个查询问题，请你生成一段 200~400 字的假设答案段落。

        要求：
        - 段落内容应该像真实文档中的陈述句，而不是对话式回答。
        - 包含与问题相关的关键术语、概念和可能的实体名称。
        - 即使你不确定真实答案，也请根据问题合理推测，生成语义上接近真实答案的文本。
        - 不要只重复问题，要扩展出可能的背景、原因、细节。

        必须调用 generate_hypothetical_document 工具输出结果。
        "#;

        let mut tool_manager = ToolManager::new();
        tool_manager.register_tool(Box::new(GenerateHypotheticalDocumentTool));

        let inner = ToolAgent::new("hyde_generator", llm, system_prompt, tool_manager);

        Self { inner }
    }

    /// 根据 query 生成假设答案文档。
    pub async fn generate(&mut self, query: &str) -> Result<String, String> {
        let result = self.inner.run(query).await?;
        Ok(result.hypothetical_document)
    }
}

/// 子 agent 唯一拥有的工具：只提供 schema，实际不执行任何操作。
///
/// 真正的“执行”发生在 LLM 侧——模型根据 schema 生成参数，
/// `ToolAgent` 再把返回值反序列化为结构化结果。
struct GenerateHypotheticalDocumentTool;

#[async_trait::async_trait]
impl Tool for GenerateHypotheticalDocumentTool {
    fn name(&self) -> &str {
        "generate_hypothetical_document"
    }

    fn description(&self) -> &str {
        "根据用户查询生成一段假设答案文档"
    }

    fn parameters_schema(&self) -> types::FunctionParameters {
        types::FunctionParameters {
            schema_type: types::JSONSchemaType::Object,
            properties: Some(HashMap::from([(
                "hypothetical_document".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some(
                        "200~400 字的假设答案段落，用于向量检索".to_string(),
                    ),
                    ..Default::default()
                }),
            )])),
            required: Some(vec!["hypothetical_document".to_string()]),
        }
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        serde_json::to_string(&args)
            .map_err(|e| format!("[GenerateHypotheticalDocumentTool] serialize args failed: {}", e))
    }
}
