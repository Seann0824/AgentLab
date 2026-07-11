use std::collections::HashMap;

use sqlx::PgPool;

use crate::base::llm::AgentsLLM;
use crate::services::rag_service::RagService;
use crate::tools::rag::chunking::{self, Paragraph};
use crate::tools::types::Tool;

/// RAG 资料库 Tool：面向 LLM 提供文档索引与语义检索能力。
pub struct RagTool {
    service: Option<RagService>,
    default_namespace: Option<String>,
}

impl RagTool {
    /// 创建一个不带索引器的 RagTool，仅用于本地文本处理操作。
    pub fn new() -> Self {
        Self {
            service: None,
            default_namespace: None,
        }
    }

    /// 使用已有的 RagService 创建 RagTool。
    pub fn with_service(service: RagService) -> Self {
        Self {
            service: Some(service),
            default_namespace: None,
        }
    }

    /// 便捷方法：用默认 Ollama embedder + PG 连接创建 RagTool。
    pub fn with_default_embedder(db: PgPool, llm: AgentsLLM) -> Self {
        Self::with_service(RagService::with_default_embedder(db, llm))
    }

    /// 设置默认 namespace；当模型调用 search 未显式指定 namespace 时使用。
    pub fn with_default_namespace(mut self, namespace: Option<String>) -> Self {
        self.default_namespace = namespace;
        self
    }

    fn service(&self) -> Result<&RagService, String> {
        self.service
            .as_ref()
            .ok_or_else(|| "RagTool 未初始化索引器".to_string())
    }

    /// 读取 Markdown 文件内容。
    pub fn get_markdown_content(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {}: {}", path, e))
    }

    pub fn split_paragraphs_with_headings(&self, text: String) -> Vec<Paragraph> {
        chunking::split_paragraphs_with_headings(text)
    }

    pub fn chunk_paragraphs(
        &self,
        paragraphs: Vec<Paragraph>,
        chunk_tokens: usize,
        overlap_tokens: usize,
    ) -> Vec<Paragraph> {
        chunking::chunk_paragraphs(paragraphs, chunk_tokens, overlap_tokens)
    }

    pub fn approx_token_len(&self, content: &str) -> usize {
        chunking::approx_token_len(content)
    }

    pub fn preprocess_markdown_for_embedding(&self, content: &str) -> String {
        use crate::tools::rag::markdown::preprocess_markdown_for_embedding;
        preprocess_markdown_for_embedding(content)
    }
}

impl Default for RagTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for RagTool {
    fn name(&self) -> &str {
        "rag"
    }

    fn description(&self) -> &str {
        "RAG 知识库工具：使用 search 在指定 namespace 下进行语义检索。可用的 namespace 会在 system prompt 中列出；如果用户已手动选中某个 namespace，则默认使用该 namespace。"
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let mut properties = HashMap::new();
        properties.insert(
            "action".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("操作类型：add_document 或 search".to_string()),
                ..Default::default()
            }),
        );
        properties.insert(
            "file_path".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("add_document 时必填：Markdown 文件路径".to_string()),
                ..Default::default()
            }),
        );
        properties.insert(
            "query".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("search 时必填：查询问题".to_string()),
                ..Default::default()
            }),
        );
        properties.insert(
            "namespace".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("search 时可选：要检索的 namespace；不填则使用 system prompt 中提示的默认 namespace".to_string()),
                ..Default::default()
            }),
        );
        openai_api_rs::v1::types::FunctionParameters {
            schema_type: openai_api_rs::v1::types::JSONSchemaType::Object,
            properties: Some(properties),
            required: Some(vec!["action".to_string()]),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().unwrap_or("").to_string();

        match action.as_str() {
            "add_document" => {
                let path = args["file_path"].as_str().unwrap_or("").to_string();
                if path.is_empty() {
                    return Err("add_document 操作需要 file_path 参数".to_string());
                }

                let result = self
                    .service()?
                    .index_document(&path, "default", 512, 64)
                    .await
                    .map_err(|e| e.to_string())?;
                if result.already_exists {
                    Ok("该文档已索引，无需重复上传".to_string())
                } else {
                    Ok(format!("索引完成，共 {} 个 chunk", result.chunks))
                }
            }
            "search" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                if query.is_empty() {
                    return Err("search 操作需要 query 参数".to_string());
                }

                let namespace = args["namespace"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or(self.default_namespace.clone());

                let results = self
                    .service()?
                    .search(&query, namespace.as_deref(), 5)
                    .await
                    .map_err(|e| e.to_string())?;

                let formatted: Vec<String> = results
                    .iter()
                    .enumerate()
                    .map(|(i, (score, chunk))| {
                        format!(
                            "[{}] distance={:.3}\n来源: {}\n标题路径: {}\n{}",
                            i + 1,
                            score,
                            chunk.source,
                            chunk.heading_path.as_deref().unwrap_or("无"),
                            chunk.content
                        )
                    })
                    .collect();

                Ok(format!(
                    "检索到 {} 条相关 chunk：\n\n{}",
                    formatted.len(),
                    formatted.join("\n\n")
                ))
            }
            _ => Err(format!("不支持的 action: {}", action)),
        }
    }
}
