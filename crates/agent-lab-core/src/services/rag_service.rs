use std::sync::Arc;

use pgvector::Vector;
use sha2::{Digest, Sha256};
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

/// 文档索引结果。
#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct IndexDocumentResult {
    pub chunks: usize,
    pub already_exists: bool,
}

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

impl Clone for RagService {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            embedder: Arc::clone(&self.embedder),
            dimension: self.dimension,
            query_expander: self.query_expander.clone(),
            hyde_generator: self.hyde_generator.clone(),
        }
    }
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
    ///
    /// 同一 namespace 下，若文件 hash 与已索引文档相同则跳过；
    /// 若 hash 不同则清空原 namespace 后重建索引。
    pub async fn index_document(
        &self,
        file_path: &str,
        namespace: &str,
        chunk_tokens: usize,
        overlap_tokens: usize,
    ) -> Result<IndexDocumentResult, AgentLabError> {
        let text = std::fs::read_to_string(file_path)
            .map_err(|e| ServiceError::external(format!("failed to read {}: {}", file_path, e)))?;
        self.index_document_content(&text, file_path, namespace, chunk_tokens, overlap_tokens)
            .await
    }

    /// 将 Markdown 内容索引到指定 namespace。
    ///
    /// 同一 namespace 下，若内容 hash 与已索引文档相同则跳过；
    /// 若 hash 不同则清空原 namespace 后重建索引。
    pub async fn index_document_content(
        &self,
        content: &str,
        source: &str,
        namespace: &str,
        chunk_tokens: usize,
        overlap_tokens: usize,
    ) -> Result<IndexDocumentResult, AgentLabError> {
        if content.is_empty() {
            return Err(ServiceError::invalid_argument(
                "content is empty",
            ))?;
        }

        let content_hash = compute_content_hash(content);

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction begin failed: {}", e)))?;

        let existing_hash: Option<String> = sqlx::query_scalar(
            "SELECT content_hash FROM rag_documents WHERE namespace = $1"
        )
        .bind(namespace)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| ServiceError::external(format!("[RagService] query document hash failed: {}", e)))?;

        if existing_hash.as_deref() == Some(content_hash.as_str()) {
            tx.commit()
                .await
                .map_err(|e| ServiceError::external(format!("[RagService] transaction commit failed: {}", e)))?;
            return Ok(IndexDocumentResult {
                chunks: 0,
                already_exists: true,
            });
        }

        // hash 不同或不存在：清空原数据并重建索引
        sqlx::query("DELETE FROM rag_chunks WHERE namespace = $1")
            .bind(namespace)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] clear chunks failed: {}", e)))?;

        sqlx::query(
            r#"
            INSERT INTO rag_documents (namespace, source, content_hash)
            VALUES ($1, $2, $3)
            ON CONFLICT (namespace) DO UPDATE SET
                source = EXCLUDED.source,
                content_hash = EXCLUDED.content_hash,
                created_at = NOW()
            "#
        )
        .bind(namespace)
        .bind(source)
        .bind(&content_hash)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServiceError::external(format!("[RagService] upsert document record failed: {}", e)))?;

        let paragraphs = chunking::split_paragraphs_with_headings(content.to_string());
        let chunks = chunking::chunk_paragraphs(paragraphs, chunk_tokens, overlap_tokens);
        let count = chunks.len();

        self.index_chunks_in_tx(chunks, source, namespace, 8, &mut tx).await?;

        tx.commit()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction commit failed: {}", e)))?;

        Ok(IndexDocumentResult {
            chunks: count,
            already_exists: false,
        })
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
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction begin failed: {}", e)))?;
        self.index_chunks_in_tx(chunks, source, namespace, batch_size, &mut tx)
            .await?;
        tx.commit()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction commit failed: {}", e)))?;
        Ok(())
    }

    /// 在已有事务中将 RAG chunk 预处理后生成 embedding，批量写入 `rag_chunks`。
    async fn index_chunks_in_tx(
        &self,
        chunks: Vec<Paragraph>,
        source: &str,
        namespace: &str,
        batch_size: usize,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
                self.insert_batch_in_tx(&batch, tx).await?;
                batch.clear();
            }
        }

        if !batch.is_empty() {
            self.insert_batch_in_tx(&batch, tx).await?;
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

    /// 列出所有已索引的 namespace。
    pub async fn list_namespaces(&self) -> Result<Vec<String>, AgentLabError> {
        let namespaces: Vec<String> = sqlx::query_scalar(
            "SELECT namespace FROM rag_documents ORDER BY namespace"
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| ServiceError::external(format!("[RagService] list namespaces failed: {}", e)))?;
        Ok(namespaces)
    }

    /// 删除某个 namespace 及其所有 chunk。
    pub async fn delete_namespace(&self, namespace: &str) -> Result<u64, AgentLabError> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction begin failed: {}", e)))?;

        let chunks_deleted = sqlx::query("DELETE FROM rag_chunks WHERE namespace = $1")
            .bind(namespace)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] delete chunks failed: {}", e)))?
            .rows_affected();

        let _ = sqlx::query("DELETE FROM rag_documents WHERE namespace = $1")
            .bind(namespace)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] delete document record failed: {}", e)))?;

        tx.commit()
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] transaction commit failed: {}", e)))?;

        Ok(chunks_deleted)
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

    async fn insert_batch_in_tx(
        &self,
        batch: &[RagChunk],
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), AgentLabError> {
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
            .execute(&mut **tx)
            .await
            .map_err(|e| ServiceError::external(format!("[RagService] insert failed: {}", e)))?;
        }

        Ok(())
    }
}

fn compute_content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}
