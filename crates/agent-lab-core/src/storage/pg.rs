use serde_json::Value;
use sqlx::{postgres::PgRow, PgPool, Row};
use pgvector::Vector;

use crate::storage::error::StorageError;
use crate::tools::memory::base::{MemoryConfig, MemoryItem};

#[derive(Clone)]
pub struct PgStore {
    #[allow(dead_code)]
    config: MemoryConfig,
    db: PgPool,
}

impl PgStore {
    pub fn new(config: MemoryConfig, db: PgPool) -> Self {
        Self {
            config,
            db,
        }
    }

    pub async fn add(
        &mut self,
        memory_item: MemoryItem,
        embedding: Vec<f32>,
    ) -> Result<(), StorageError> {
        let pg_vector = Vector::from(embedding);

        sqlx::query(r#"
            INSERT INTO memories (
                memory_id, user_id, memory_type, content, embedding,
                importance, timestamp, session_id, properties
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#)
            .bind(&memory_item.id)
            .bind(&memory_item.user_id)
            .bind(&memory_item.memory_type)
            .bind(&memory_item.content)
            .bind(pg_vector)
            .bind(memory_item.importance)
            .bind(memory_item.timestamp)
            .bind(memory_item.session_id)
            .bind(&memory_item.metadata)
            .execute(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] insert failed: {}", e)))?;

        Ok(())
    }

    pub async fn search_similar(
        &self,
        pg_vector: Vector,
        memory_type: &str,
        user_id: Option<&str>,
        session_id: Option<&str>,
        importance_threshold: Option<f64>,
        time_range: Option<(i64, i64)>,
        limit: usize,
    ) -> Result<Vec<(f64, MemoryItem)>, StorageError> {
        let (start_time, end_time) = match time_range {
            Some((start, end)) => (Some(start), Some(end)),
            None => (None, None),
        };

        let rows = sqlx::query(r#"
            SELECT
                memory_id, user_id, memory_type, content, importance,
                timestamp, session_id, properties,
                embedding <=> $1 AS score
            FROM memories
            WHERE memory_type = $2
              AND (user_id = $3 OR $3 IS NULL)
              AND (session_id = $4 OR $4 IS NULL)
              AND (importance >= $5 OR $5 IS NULL)
              AND (timestamp >= $6 OR $6 IS NULL)
              AND (timestamp <= $7 OR $7 IS NULL)
            ORDER BY embedding <=> $1
            LIMIT $8
        "#)
            .bind(pg_vector)
            .bind(memory_type)
            .bind(user_id)
            .bind(session_id)
            .bind(importance_threshold)
            .bind(start_time)
            .bind(end_time)
            .bind(limit as i64)
            .fetch_all(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] search failed: {}", e)))?;

        let results: Vec<(f64, MemoryItem)> = rows
            .into_iter()
            .map(|row| {
                let memory_item = MemoryItem {
                    id: row.get("memory_id"),
                    user_id: row.get("user_id"),
                    memory_type: row.get("memory_type"),
                    content: row.get("content"),
                    timestamp: row.get("timestamp"),
                    importance: row.get::<f64, _>("importance"),
                    session_id: row.get("session_id"),
                    metadata: row.get::<Option<Value>, _>("properties")
                        .unwrap_or_else(|| Value::Object(Default::default())),
                };
                let score: f64 = row.get("score");
                (score, memory_item)
            })
            .collect();

        Ok(results)
    }

    pub async fn keyword_search(
        &self,
        query: &str,
        memory_type: &str,
        user_id: Option<&str>,
        session_id: Option<&str>,
        importance_threshold: Option<f64>,
        time_range: Option<(i64, i64)>,
    ) -> Result<Vec<MemoryItem>, StorageError> {
        let pattern = format!("%{}%", query);

        let (start_time, end_time) = match time_range {
            Some((start, end)) => (Some(start), Some(end)),
            None => (None, None),
        };

        let rows = sqlx::query(r#"
            SELECT
                memory_id, user_id, memory_type, content, importance,
                timestamp, session_id, properties
            FROM memories
            WHERE memory_type = $1
              AND content ILIKE $2
              AND (user_id = $3 OR $3 IS NULL)
              AND (session_id = $4 OR $4 IS NULL)
              AND (importance >= $5 OR $5 IS NULL)
              AND (timestamp >= $6 OR $6 IS NULL)
              AND (timestamp <= $7 OR $7 IS NULL)
        "#)
            .bind(memory_type)
            .bind(pattern)
            .bind(user_id)
            .bind(session_id)
            .bind(importance_threshold)
            .bind(start_time)
            .bind(end_time)
            .fetch_all(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] keyword search failed: {}", e)))?;

        let results: Vec<MemoryItem> = rows
            .into_iter()
            .map(|row| MemoryItem {
                id: row.get("memory_id"),
                user_id: row.get("user_id"),
                memory_type: row.get("memory_type"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                importance: row.get::<f64, _>("importance"),
                session_id: row.get("session_id"),
                metadata: row.get::<Option<Value>, _>("properties")
                    .unwrap_or_else(|| Value::Object(Default::default())),
            })
            .collect();

        Ok(results)
    }

    pub async fn get(&self, memory_id: &str) -> Result<Option<MemoryItem>, StorageError> {
        let row = sqlx::query(r#"
            SELECT
                memory_id, user_id, memory_type, content, importance,
                timestamp, session_id, properties
            FROM memories
            WHERE memory_id = $1
        "#)
            .bind(memory_id)
            .fetch_optional(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] get failed: {}", e)))?;

        Ok(row.map(|r| Self::row_to_memory_item(&r)))
    }

    pub async fn update(
        &self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f64>,
        metadata: Option<&Value>,
        new_embedding: Option<Vector>,
    ) -> Result<bool, StorageError> {
        let updated = sqlx::query(r#"
            UPDATE memories
            SET content = COALESCE($2, content),
                importance = COALESCE($3, importance),
                properties = COALESCE($4, properties),
                embedding = COALESCE($5, embedding)
            WHERE memory_id = $1
        "#)
            .bind(memory_id)
            .bind(content)
            .bind(importance)
            .bind(metadata)
            .bind(new_embedding)
            .execute(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] update failed: {}", e)))?
            .rows_affected();

        Ok(updated > 0)
    }

    pub async fn delete(&self, memory_id: &str) -> Result<bool, StorageError> {
        let deleted = sqlx::query("DELETE FROM memories WHERE memory_id = $1")
            .bind(memory_id)
            .execute(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] delete failed: {}", e)))?
            .rows_affected();

        Ok(deleted > 0)
    }

    pub async fn list_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<MemoryItem>, StorageError> {
        let limit = limit.unwrap_or(10_000);

        let rows = sqlx::query(r#"
            SELECT
                memory_id, user_id, memory_type, content, importance,
                timestamp, session_id, properties
            FROM memories
            WHERE memory_type = $1
              AND (user_id = $2 OR $2 IS NULL)
            ORDER BY timestamp DESC
            LIMIT $3
        "#)
            .bind(memory_type)
            .bind(user_id)
            .bind(limit)
            .fetch_all(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] list failed: {}", e)))?;

        Ok(rows.iter().map(Self::row_to_memory_item).collect())
    }

    pub async fn clear_by_type(&self, memory_type: &str) -> Result<u64, StorageError> {
        let deleted = sqlx::query("DELETE FROM memories WHERE memory_type = $1")
            .bind(memory_type)
            .execute(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] clear failed: {}", e)))?
            .rows_affected();

        Ok(deleted)
    }

    pub async fn count_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<i64, StorageError> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memories WHERE memory_type = $1 AND (user_id = $2 OR $2 IS NULL)"
        )
            .bind(memory_type)
            .bind(user_id)
            .fetch_one(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] count failed: {}", e)))?;

        Ok(count.0)
    }

    pub async fn avg_importance_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, StorageError> {
        let avg: (Option<f64>,) = sqlx::query_as(
            "SELECT AVG(importance) FROM memories WHERE memory_type = $1 AND (user_id = $2 OR $2 IS NULL)"
        )
            .bind(memory_type)
            .bind(user_id)
            .fetch_one(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] avg importance failed: {}", e)))?;

        Ok(avg.0)
    }

    pub async fn time_span_days_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, StorageError> {
        let span: (Option<f64>,) = sqlx::query_as(
            "SELECT EXTRACT(EPOCH FROM (MAX(to_timestamp(timestamp)) - MIN(to_timestamp(timestamp)))) / 86400.0
             FROM memories
             WHERE memory_type = $1 AND (user_id = $2 OR $2 IS NULL)"
        )
            .bind(memory_type)
            .bind(user_id)
            .fetch_one(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[PgStore] time span failed: {}", e)))?;

        Ok(span.0)
    }

    fn row_to_memory_item(row: &PgRow) -> MemoryItem {
        MemoryItem {
            id: row.get("memory_id"),
            user_id: row.get("user_id"),
            memory_type: row.get("memory_type"),
            content: row.get("content"),
            timestamp: row.get("timestamp"),
            importance: row.get::<f64, _>("importance"),
            session_id: row.get("session_id"),
            metadata: row.get::<Option<Value>, _>("properties")
                .unwrap_or_else(|| Value::Object(Default::default())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use super::*;
    use crate::db::get_db_client;
    use crate::storage::embedder::Embedder;

    struct MockEmbedder;

    #[async_trait::async_trait]
    impl Embedder for MockEmbedder {
        async fn encode(&self, _text: &str) -> Result<Vec<f32>, String> {
            // 表 memories.embedding 要求 768 维
            Ok(vec![0.1f32; 768])
        }
    }

    #[tokio::test]
    async fn test_pg_store_add() {
        dotenvy::dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL not set");
        let db = get_db_client(&database_url).await;
        let config = MemoryConfig::new();
        let embedder: Arc<dyn Embedder + Send + Sync> = Arc::new(MockEmbedder);
        let mut store = PgStore::new(config, db.clone());

        let memory_item = MemoryItem::new(
            "test_user".to_string(),
            "episodic".to_string(),
            "test content for PgStore::add".to_string(),
            0.8,
            serde_json::json!({"key": "value"}),
        );

        // 清理可能遗留的测试数据
        sqlx::query("DELETE FROM memories WHERE memory_id = $1")
            .bind(&memory_item.id)
            .execute(&db)
            .await
            .unwrap();

        let embedding = embedder.encode(&memory_item.content).await.unwrap();
        let result = store.add(memory_item.clone(), embedding).await;
        assert!(result.is_ok(), "PgStore::add should return Ok");

        let row: (String, String, f64) = sqlx::query_as(
            "SELECT user_id, content, importance FROM memories WHERE memory_id = $1"
        )
        .bind(&memory_item.id)
        .fetch_one(&db)
        .await
        .expect("inserted memory should be found in database");

        assert_eq!(row.0, memory_item.user_id);
        assert_eq!(row.1, memory_item.content);
        assert!((row.2 - memory_item.importance).abs() < f64::EPSILON);

        // 清理测试数据
        sqlx::query("DELETE FROM memories WHERE memory_id = $1")
            .bind(&memory_item.id)
            .execute(&db)
            .await
            .unwrap();
    }
}
