use std::sync::Arc;

use pgvector::Vector;
use sqlx::{PgPool, Row};

use crate::tools::memory::storage::embedder::Embedder;
use crate::tools::memory::storage::OllamaEmbedder;
use crate::tools::rag::chunking::Paragraph;
use crate::tools::rag::hyde::HydeAgent;
use crate::tools::rag::markdown::preprocess_markdown_for_embedding;
use crate::tools::rag::query_expansion::QueryExpansionAgent;

/// RAG 全局资料库中的一条 chunk 记录。
/// 与 memory 的 `memories` 表解耦，字段更精简，面向文档检索场景。
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

/// RAG 索引器：负责把分块后的文本生成 embedding 并写入独立的 `rag_chunks` 表。
///
/// 与 memory 体系解耦：`rag_chunks` 是全局资料库，字段精简，
/// namespace 仅用于多资料库隔离，不对应具体用户。
#[derive(Clone)]
pub struct RagIndex {
    pub(crate) db: PgPool,
    pub(crate) embedder: Arc<dyn Embedder + Send + Sync>,
    pub(crate) dimension: usize,
    pub(crate) query_expander: Option<Arc<tokio::sync::Mutex<QueryExpansionAgent>>>,
    pub(crate) hyde_generator: Option<Arc<tokio::sync::Mutex<HydeAgent>>>,
}

impl RagIndex {
    pub fn new(db: PgPool, embedder: Arc<dyn Embedder + Send + Sync>, dimension: usize) -> Self {
        Self {
            db,
            embedder,
            dimension,
            query_expander: None,
            hyde_generator: None,
        }
    }

    /// 使用默认的 Ollama embedder 创建索引器，并启用 MQE + HyDE。
    /// 默认维度 768，与 `init_pg.sql` 中的 rag_chunks.embedding VECTOR(768) 对应。
    pub fn with_default_embedder(db: PgPool) -> Self {
        let embedder = Arc::new(OllamaEmbedder::new(None, None));
        let query_expander = Some(Arc::new(tokio::sync::Mutex::new(
            QueryExpansionAgent::from_env(),
        )));
        let hyde_generator = Some(Arc::new(tokio::sync::Mutex::new(HydeAgent::from_env())));
        Self {
            db,
            embedder,
            dimension: 768,
            query_expander,
            hyde_generator,
        }
    }

    /// 是否启用 MQE 查询扩展。
    pub fn with_query_expansion(mut self, agent: QueryExpansionAgent) -> Self {
        self.query_expander = Some(Arc::new(tokio::sync::Mutex::new(agent)));
        self
    }

    /// 是否启用 HyDE 假设文档嵌入。
    pub fn with_hyde(mut self, agent: HydeAgent) -> Self {
        self.hyde_generator = Some(Arc::new(tokio::sync::Mutex::new(agent)));
        self
    }

    /// 将 RAG chunk 预处理后生成 embedding，批量写入 `rag_chunks`。
    ///
    /// - `source`: 来源文档标识（如文件路径）
    /// - `namespace`: RAG 命名空间，用于多资料库隔离
    /// - `batch_size`: 每批写入的 chunk 数量
    pub async fn index_chunks(
        &self,
        chunks: Vec<Paragraph>,
        source: &str,
        namespace: &str,
        batch_size: usize,
    ) -> Result<(), String> {
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
                .map_err(|e| format!("[RagIndex] embedding failed: {}", e))?;
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

    pub(crate) async fn search_single(
        &self,
        query: &str,
        namespace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(f64, RagChunk)>, String> {
        let mut embedding = self
            .embedder
            .encode(query)
            .await
            .map_err(|e| format!("[RagIndex] query embedding failed: {}", e))?;
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
        .map_err(|e| format!("[RagIndex] search failed: {}", e))?;

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

    /// 清空某个 namespace 下的所有 chunk，便于重新索引同一资料库。
    pub async fn clear_namespace(&self, namespace: &str) -> Result<u64, String> {
        let deleted = sqlx::query("DELETE FROM rag_chunks WHERE namespace = $1")
            .bind(namespace)
            .execute(&self.db)
            .await
            .map_err(|e| format!("[RagIndex] clear namespace failed: {}", e))?
            .rows_affected();
        Ok(deleted)
    }

    async fn insert_batch(&self, batch: &[RagChunk]) -> Result<(), String> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| format!("[RagIndex] transaction begin failed: {}", e))?;

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
            .map_err(|e| format!("[RagIndex] insert failed: {}", e))?;
        }

        tx.commit()
            .await
            .map_err(|e| format!("[RagIndex] transaction commit failed: {}", e))?;

        Ok(())
    }
}
