use std::collections::HashMap;
use openai_api_rs::v1::types;
use serpapi::serpapi::Client;
use crate::tools::types::{Tool, ToolError};

pub struct WebSearch {
    api_key: String,
}

impl WebSearch {
    /// 创建一个使用 SerpApi 的网页搜索工具。
    /// `api_key` 对应 SerpApi 的 API Key。
    pub fn serpapi(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

#[async_trait::async_trait]
impl Tool for WebSearch  {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "一个网页搜索引擎。当你需要回答关于时事、事实以及在你的知识库中找不到的信息时，应使用此工具。"
    }
    
    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let properties = HashMap::from([
            (
                "query".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::String),
                    description: Some("搜索关键词".to_string()),
                    ..Default::default()
                }),
            ),
        ]);
        openai_api_rs::v1::types::FunctionParameters {
            schema_type: openai_api_rs::v1::types::JSONSchemaType::Object,
            properties: Some(properties),
            required: Some(vec!["query".to_string()]),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let query = args["query"].as_str().unwrap_or("").to_string();
        if self.api_key.is_empty() {
            return Err(ToolError::InvalidArgument(
                "WebSearch api_key 未配置".to_string(),
            ));
        }

        let client = Client::new(HashMap::from([
            ("api_key".to_string(), self.api_key.clone()),
            ("engine".to_string(), "google".to_string()),
        ]))
        .map_err(|e| ToolError::external("SerpApi", format!("初始化失败: {}", e)))?;

        let query_params = HashMap::from([
            ("q".to_string(), query.clone()),
            ("gl".to_string(), "cn".into()),
            ("hl".to_string(), "zh-cn".into()),
        ]);

        match client.search(query_params).await {
            Ok(results) => {
                if let Some(answer_box_list) = results.get("answer_box_list") {
                    return Ok(format!("\n{}", answer_box_list));
                }

                if let Some(answer_box) = results.get("answer_box")
                    && let Some(answer) = answer_box.get("answer")
                {
                    return Ok(answer.to_string());
                }

                if let Some(knowledge_graph) = results.get("knowledge_graph")
                    && let Some(description) = knowledge_graph.get("description")
                {
                    return Ok(description.to_string());
                }

                if let Some(organic_results) = results["organic_results"].as_array()
                    && !organic_results.is_empty()
                {
                    // 返回前3
                    let snippets = organic_results
                        .iter()
                        .enumerate()
                        .take(3)
                        .map(|(i, res)| format!("{} {}\n{}", i + 1, res["title"], res["snippet"]))
                        .collect::<Vec<_>>()
                        .join("");

                    return Ok(format!("\n\n{}", snippets));
                }

                Ok(format!("对不起，没有找到关于 {} 的信息。", query))
            }
            Err(e) => Err(ToolError::external("SerpApi", e.to_string())),
        }
    }
}