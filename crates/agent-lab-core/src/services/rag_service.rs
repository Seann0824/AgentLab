use std::sync::Arc;

use pgvector::Vector;
use sqlx::{PgPool, Row};

use crate::base::llm::AgentsLLM;
use crate::error::AgentLabError;
use crate::services::ServiceError;
use crate::storage::embedder::Embedder;
use crate::storage::OllamaEmbedder;
use crate::tools::rag::chunking::{self, Paragraph};
use crate::tools::rag::hyde::HydeAgent;
use crate::tools::rag::markdown::preprocess_markdown_for_embedding;
use crate::tools::rag::query_expansion::QueryExpansionAgent;
use crate::tools::rag::retrieval;

/// RAG 全局资料库中的一条 chunk 记录。
#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct RagChunk {
    pub id: String,
    pub namespace: String,
    pub source: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub heading_path: Option<String>,
    pub start: usize,
    pub end: usize,
    pub chunk_index: usize,
    pub metadata: serde_json::Value,
}

/// RAG 业务服务：负责文档索引与语义检索。
pub struct RagService {
    db: PgPool,
    embedder: Arc<dyn Embedder + Send + Sync>,
    dimension: usize,
    pub(crate) query_expander: Option<Arc<tokio::sync::Mutex<QueryExpansionAgent>>>,
    pub(crate) hyde_generator: Option<Arc<tokio::sync::Mutex<HydeAgent>>>,
}

impl RagService {
    pub fn new(db: PgPool, embedder: Arc<dyn Embedder + Send + Sync>, dimension: usize) -> Self {
        Self {
            db,
            embedder,
            dimension,
            query_expander: None,
            hyde_generator: None,
        }
    }

    /// 使用默认的 Ollama embedder 创建服务，并启用 MQE + HyDE。
    pub fn with_default_embedder(db: PgPool, llm: AgentsLLM) -> Self {
        let embedder = Arc::new(OllamaEmbedder::new(None, None));
        let query_expander = Some(Arc::new(tokio::sync::Mutex::new(
            QueryExpansionAgent::new(llm.clone()),
        )));
        let hyde_generator = Some(Arc::new(tokio::sync::Mutex::new(HydeAgent::new(llm))));
        Self {
            db,
            embedder,
            dimension: 768,
            query_expander,
            hyde_generator,
        }
    }

    /// 读取 Markdown 文件并索引到指定 namespace。
    pub async fn index_document(
        &self,
        file_path: &str,
        namespace: &str,
        chunk_tokens: usize,
        overlap_tokens: usize,
    ) -> Result<usize, AgentLabError> {
        let text = std::fs::read_to_string(file_path)
            .map_err(|e| ServiceError::external(format!("failed to read {}: {}", file_path, e)))?;
        if text.is_empty() {
            return Err(ServiceError::invalid_argument(
                "file is empty or could not be read",
            ))?;
        }

        let paragraphs = chunking::split_paragraphs_with_headings(text);
        let chunks = chunking::chunk_paragraphs(paragraphs, chunk_tokens, overlap_tokens);
        let count = chunks.len();

        self.clear_namespace(namespace).await?;
        self.index_chunks(chunks, file_path, namespace, 8).await?;

        Ok(count)
    }

    /// 语义检索：支持 HyDE + MQE 增强。
    pub async fn search(
        &self,
        query: &str,
        namespace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(f64, RagChunk)>, AgentLabError> {
        retrieval::search(self, query, namespace, limit).await
    }

    /// 将 RAG chunk 预处理后生成 embedding，批量写入 `rag_chunks`。
    pub async fn index_chunks(
        &self,
        chunks: Vec<Paragraph>,
        source: &str,
        namespace: &str,
        batch_size: usize,
    ) -> Result<(), AgentLabError> {
        if chunks.is_empty() {
            return Ok(());
        }

        let processed: Vec<(Paragraph, String)> = chunks
            .into_iter()
            .map(|chunk| {
                let processed = preprocess_markdown_for_embedding(&chunk.content);
                (chunk, processed)
            })
            .collect();

        let mut batch: Vec<RagChunk> = Vec::with_capacity(batch_size);

        for (chunk_index, (chunk, text)) in processed.into_iter().enumerate() {
            let mut embedding = self
                .embedder
                .encode(&text)
                .await
                .map_err(|e| ServiceError::embedding(format!("[RagService] embedding failed: {}", e)))?;
            self.normalize_embedding(&mut embedding);

            batch.push(RagChunk {
                id: uuid::Uuid::new_v4().to_string(),
                namespace: namespace.to_string(),
                source: source.to_string(),
                content: text,
                embedding,
                heading_path: chunk.heading_path,
                start: chunk.start,
                end: chunk.end,
                chunk_index,
                metadata: serde_json::json!({
                    "original_content": chunk.content,
                }),
            });

            if batch.len() >= batch_size {
                self.insert_batch(&batch).await?;
                batch.clear();
            }
        }

        if !batch.is_empty() {
            self.insert_batch(&batch).await?;
        }

        Ok(())
    }

    /// 清空某个 namespace 下的所有 chunk。
    pub async fn clear_namespace(&self, namespace: &str) -> Result<u64, AgentLabError> {
        let deleted = sqlx::query("DELETE FROM rag_chunks WHERE namespace = $1")
            .bind(namespace)
            .execute(&self.db)
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] clear namespace failed: {}", e)))?
            .rows_affected();
        Ok(deleted)
    }

    /// 单查询向量检索，供 retrieval 模块调用。
    pub(crate) async fn search_single(
        &self,
        query: &str,
        namespace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(f64, RagChunk)>, AgentLabError> {
        let mut embedding = self
            .embedder
            .encode(query)
            .await
            .map_err(|e| ServiceError::embedding(format!("[RagService] query embedding failed: {}", e)))?;
        self.normalize_embedding(&mut embedding);
        let pg_vector = Vector::from(embedding);

        let rows = sqlx::query(
            r#"
            SELECT
                id, namespace, source, content, embedding,
                heading_path, start_pos, end_pos, chunk_index, metadata,
                embedding <=> $1 AS score
            FROM rag_chunks
            WHERE namespace = $2 OR $2 IS NULL
            ORDER BY embedding <=> $1
            LIMIT $3
            "#,
        )
        .bind(pg_vector)
        .bind(namespace)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await
        .map_err(|e| ServiceError::external(format!("[RagService] search failed: {}", e)))?;

        let results = rows
            .into_iter()
            .map(|row| {
                let chunk = RagChunk {
                    id: row.get("id"),
                    namespace: row.get("namespace"),
                    source: row.get("source"),
                    content: row.get("content"),
                    embedding: row
                        .get::<Option<Vector>, _>("embedding")
                        .map(|v| v.to_vec())
                        .unwrap_or_default(),
                    heading_path: row.get("heading_path"),
                    start: row.get::<i64, _>("start_pos") as usize,
                    end: row.get::<i64, _>("end_pos") as usize,
                    chunk_index: row.get::<i32, _>("chunk_index") as usize,
                    metadata: row
                        .get::<Option<serde_json::Value>, _>("metadata")
                        .unwrap_or_else(|| serde_json::json!({})),
                };
                let score: f64 = row.get("score");
                (score, chunk)
            })
            .collect();

        Ok(results)
    }

    pub(crate) fn normalize_embedding(&self, embedding: &mut Vec<f32>) {
        if embedding.len() < self.dimension {
            embedding.extend(vec![0.0f32; self.dimension - embedding.len()]);
        } else if embedding.len() > self.dimension {
            embedding.truncate(self.dimension);
        }
    }

    async fn insert_batch(&self, batch: &[RagChunk]) -> Result<(), AgentLabError> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction begin failed: {}", e)))?;

        for chunk in batch {
            let pg_vector = Vector::from(chunk.embedding.clone());
            sqlx::query(
                r#"
                INSERT INTO rag_chunks (
                    id, namespace, source, content, embedding,
                    heading_path, start_pos, end_pos, chunk_index, metadata
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                "#,
            )
            .bind(&chunk.id)
            .bind(&chunk.namespace)
            .bind(&chunk.source)
            .bind(&chunk.content)
            .bind(pg_vector)
            .bind(&chunk.heading_path)
            .bind(chunk.start as i64)
            .bind(chunk.end as i64)
            .bind(chunk.chunk_index as i32)
            .bind(&chunk.metadata)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] insert failed: {}", e)))?;
        }

        tx.commit()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction commit failed: {}", e)))?;

        Ok(())
    }
}
