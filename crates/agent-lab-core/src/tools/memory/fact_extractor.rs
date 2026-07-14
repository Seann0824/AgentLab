use std::collections::HashMap;

use openai_api_rs::v1::types;
use serde_json::Value;

use crate::agent::tool_agent::ToolAgent;
use crate::base::llm::AgentsLLM;
use crate::tools::ToolManager;
use crate::tools::types::{Tool, ToolError};

/// 内部子 agent：从对话上下文中提取应被记忆的事实。
///
/// 它基于通用 `ToolAgent<T>` 实现，只注册一个 `extract_facts` 工具，
/// 通过 OpenAI 的 function calling 让模型按固定 schema 输出结构化结果。
pub struct MemoryFactExtractor {
    inner: ToolAgent<FactExtractionResult>,
}

#[derive(serde::Deserialize)]
struct FactExtractionResult {
    facts: Vec<String>,
}

impl MemoryFactExtractor {
    pub fn new(llm: AgentsLLM) -> Self {
        let system_prompt = r#"You are a Personal Information Organizer. Extract relevant facts, memories, preferences, intentions, and needs from conversations into distinct, manageable facts.

Information Types: Personal preferences, details (names, relationships, dates), plans, intentions, needs, requests, activities, health/wellness (including medical appointments, symptoms, treatments), professional, miscellaneous.

CRITICAL Rules:
1. TEMPORAL: ALWAYS extract time info (dates, relative refs like "yesterday", "last week"). Include in facts (e.g., "Went to Hawaii in May 2023" or "Went to Hawaii last year", not just "Went to Hawaii"). Preserve relative time refs for later calculation.
2. COMPLETE: Extract self-contained facts with who/what/when/where when available.
3. SEPARATE: Extract distinct facts separately, especially when they have different time periods.
4. INTENTIONS & NEEDS: ALWAYS extract user intentions, needs, and requests even without time information. Examples: "Want to book a doctor appointment", "Need to call someone", "Plan to visit a place".
5. LANGUAGE: DO NOT translate. Preserve the original language of the source text for each extracted fact. If the input is Chinese, output facts in Chinese; if English, output facts in English; if mixed-language, keep each fact in the language it appears in.

Examples:
Input: Hi.
Output: {"facts" : []}

Input: Yesterday, I met John at 3pm. We discussed the project.
Output: {"facts" : ["Met John at 3pm yesterday", "Discussed project with John yesterday"]}

Input: Last May, I went to India. Visited Mumbai and Goa.
Output: {"facts" : ["Went to India in May", "Visited Mumbai in May", "Visited Goa in May"]}

Input: I met Sarah last year and became friends. We went to movies last month.
Output: {"facts" : ["Met Sarah last year and became friends", "Went to movies with Sarah last month"]}

Input: I'm John, a software engineer.
Output: {"facts" : ["Name is John", "Is a software engineer"]}

Input: I want to book an appointment with a cardiologist.
Output: {"facts" : ["Want to book an appointment with a cardiologist"]}

Rules:
- Today 请参考用户输入开头提供的日期。
- Return JSON: {"facts": ["fact1", "fact2"]}
- Extract from user/assistant messages only
- Extract intentions, needs, and requests even without time information
- If no relevant facts, return empty list
- Output must preserve the input language (no translation)"#;

        let mut tool_manager = ToolManager::new();
        tool_manager.register_tool(Box::new(ExtractFactsTool));

        let inner = ToolAgent::new("memory_fact_extractor", llm, system_prompt, tool_manager);

        Self { inner }
    }

    /// 从对话上下文中抽取事实列表。
    ///
    /// 返回的 `Vec` 可能为空（模型认为没有值得记忆的事实时）。
    pub async fn extract(&mut self, context: &str) -> Result<Vec<String>, String> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let input = format!(
            "Today: {}\n\nExtract facts from the conversation below:\n{}",
            today, context
        );
        let result = self.inner.run(&input).await?;
        Ok(result.facts)
    }
}

/// 子 agent 唯一拥有的工具：只提供 schema，实际不执行任何操作。
///
/// 真正的“执行”发生在 LLM 侧——模型根据 schema 生成参数，业务层再解析这些参数。
struct ExtractFactsTool;

#[async_trait::async_trait]
impl Tool for ExtractFactsTool {
    fn name(&self) -> &str {
        "extract_facts"
    }

    fn description(&self) -> &str {
        "从对话上下文中抽取应被记忆的事实"
    }

    fn parameters_schema(&self) -> types::FunctionParameters {
        let fact_item = types::JSONSchemaDefine {
            schema_type: Some(types::JSONSchemaType::String),
            description: Some("一条自包含的记忆事实".to_string()),
            ..Default::default()
        };

        types::FunctionParameters {
            schema_type: types::JSONSchemaType::Object,
            properties: Some(HashMap::from([(
                "facts".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Array),
                    description: Some("抽取出的记忆事实列表".to_string()),
                    items: Some(Box::new(fact_item)),
                    ..Default::default()
                }),
            )])),
            required: Some(vec!["facts".to_string()]),
        }
    }

    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        // 该工具仅用于约束 LLM 的输出格式，执行时直接把参数原样返回，
        // 由 ToolAgent 把返回值反序列化为结构化结果。
        serde_json::to_string(&args).map_err(|e| {
            ToolError::Internal(format!(
                "[ExtractFactsTool] serialize args failed: {}",
                e
            ))
        })
    }
}
