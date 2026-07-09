use std::collections::HashMap;

use sqlx::PgPool;

use crate::base::llm::AgentsLLM;
use crate::tools::rag::chunking::{self, Paragraph};
use crate::tools::rag::index::RagIndex;
use crate::tools::rag::markdown::preprocess_markdown_for_embedding;
use crate::tools::rag::retrieval;
use crate::tools::types::Tool;

pub struct RagTool {
    index: Option<RagIndex>,
}

impl RagTool {
    /// 创建一个不带索引器的 RagTool，仅用于分块、预处理和 token 估算等本地操作。
    pub fn new() -> Self {
        Self { index: None }
    }

    /// 使用已有的 RagIndex 创建 RagTool。
    pub fn with_index(index: RagIndex) -> Self {
        Self { index: Some(index) }
    }

    /// 便捷方法：用默认 Ollama embedder + PG 连接创建 RagTool。
    /// `llm` 用于驱动查询扩展与 HyDE 子 agent。
    pub fn with_default_embedder(db: PgPool, llm: AgentsLLM) -> Self {
        Self::with_index(RagIndex::with_default_embedder(db, llm))
    }

    fn index(&self) -> Result<&RagIndex, String> {
        self.index
            .as_ref()
            .ok_or_else(|| "RagTool 未初始化索引器".to_string())
    }

    /// 把本地 Markdown 文件索引到 `rag_chunks`。
    pub async fn index_file(
        &self,
        path: &str,
        namespace: &str,
        chunk_tokens: usize,
        overlap_tokens: usize,
    ) -> Result<usize, String> {
        let text = self.get_markdown_content(path)?;
        if text.is_empty() {
            return Err("file is empty or could not be read".to_string());
        }

        let paragraphs = chunking::split_paragraphs_with_headings(text);
        let chunks = chunking::chunk_paragraphs(paragraphs, chunk_tokens, overlap_tokens);
        let count = chunks.len();

        let index = self.index()?;
        index
            .clear_namespace(namespace)
            .await
            .map_err(|e| format!("clear namespace failed: {}", e))?;
        index
            .index_chunks(chunks, path, namespace, 8)
            .await
            .map_err(|e| format!("index chunks failed: {}", e))?;

        Ok(count)
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
        preprocess_markdown_for_embedding(content)
    }
}

#[async_trait::async_trait]
impl Tool for RagTool {
    fn name(&self) -> &str {
        "rag"
    }

    fn description(&self) -> &str {
        "RAG 资料库工具：支持 add_document（索引 Markdown 文档）和 search（语义检索）。"
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

                // 内部固定参数：整个 RAG 视为统一数据库
                let count = self.index_file(&path, "default", 512, 64).await?;
                Ok(format!("索引完成，共 {} 个 chunk", count))
            }
            "search" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                if query.is_empty() {
                    return Err("search 操作需要 query 参数".to_string());
                }

                let index = self.index()?;
                // 不限制 namespace，在整个 RAG 数据库里按向量语义相似度排序
                let results = retrieval::search(index, &query, None, 5).await?;

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
