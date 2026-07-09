use serde_json::Value;
use sqlx::{postgres::PgRow, PgPool, Row};

use crate::services::chat_dto::{ChatMessage, SessionSummary};
use crate::storage::error::StorageError;

/// 会话原始记录。
#[derive(Clone)]
pub struct ChatSessionRow {
    pub id: String,
    pub user_id: String,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub metadata: Option<Value>,
}

/// 聊天会话与消息的 PG 存储层。
#[derive(Clone)]
pub struct ChatStore {
    db: PgPool,
}

impl ChatStore {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// 创建会话。
    pub async fn create_session(
        &self,
        id: &str,
        user_id: &str,
        title: Option<&str>,
        created_at: i64,
        updated_at: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO chat_sessions (id, user_id, title, created_at, updated_at, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(id)
        .bind(user_id)
        .bind(title)
        .bind(created_at)
        .bind(updated_at)
        .bind(Value::Object(Default::default()))
        .execute(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] create session failed: {}", e)))?;

        Ok(())
    }

    /// 查询会话元数据。
    pub async fn get_session(&self, session_id: &str) -> Result<Option<ChatSessionRow>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, user_id, title, created_at, updated_at, metadata
            FROM chat_sessions
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] get session failed: {}", e)))?;

        Ok(row.map(|r| Self::row_to_session(&r)))
    }

    /// 列出用户的会话摘要。
    pub async fn list_sessions(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionSummary>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, title, updated_at
            FROM chat_sessions
            WHERE user_id = $1
            ORDER BY updated_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] list sessions failed: {}", e)))?;

        Ok(rows
            .iter()
            .map(|r| SessionSummary {
                id: r.get("id"),
                title: r.get::<Option<String>, _>("title").unwrap_or_else(|| "新会话".to_string()),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    /// 更新会话标题。
    pub async fn update_session_title(
        &self,
        session_id: &str,
        title: &str,
        updated_at: i64,
    ) -> Result<bool, StorageError> {
        let updated = sqlx::query(
            r#"
            UPDATE chat_sessions
            SET title = $2, updated_at = $3
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .bind(title)
        .bind(updated_at)
        .execute(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] update title failed: {}", e)))?
        .rows_affected();

        Ok(updated > 0)
    }

    /// 更新会话更新时间。
    pub async fn touch_session(&self, session_id: &str, updated_at: i64) -> Result<bool, StorageError> {
        let updated = sqlx::query(
            r#"
            UPDATE chat_sessions
            SET updated_at = $2
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .bind(updated_at)
        .execute(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] touch session failed: {}", e)))?
        .rows_affected();

        Ok(updated > 0)
    }

    /// 删除会话（级联删除消息）。
    pub async fn delete_session(&self, session_id: &str) -> Result<bool, StorageError> {
        let deleted = sqlx::query("DELETE FROM chat_sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[ChatStore] delete session failed: {}", e)))?
            .rows_affected();

        Ok(deleted > 0)
    }

    /// 添加消息。
    pub async fn add_message(
        &self,
        session_id: &str,
        msg: &ChatMessage,
        seq: i32,
    ) -> Result<(), StorageError> {
        let tool_calls_json = msg
            .tool_calls
            .as_ref()
            .map(|tc| serde_json::to_value(tc).ok())
            .flatten();

        sqlx::query(
            r#"
            INSERT INTO chat_messages (
                id, session_id, role, content, timestamp,
                tool_call_id, tool_calls, metadata, seq
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&msg.id)
        .bind(session_id)
        .bind(&msg.role)
        .bind(&msg.content)
        .bind(msg.timestamp)
        .bind(&msg.tool_call_id)
        .bind(&tool_calls_json)
        .bind(&msg.metadata)
        .bind(seq)
        .execute(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] add message failed: {}", e)))?;

        Ok(())
    }

    /// 获取会话消息历史。
    pub async fn get_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, role, content, timestamp,
                tool_call_id, tool_calls, metadata
            FROM chat_messages
            WHERE session_id = $1
            ORDER BY seq ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] get messages failed: {}", e)))?;

        Ok(rows.iter().map(Self::row_to_message).collect())
    }

    /// 获取下一个 seq。
    pub async fn next_seq(&self, session_id: &str) -> Result<i32, StorageError> {
        let row: (Option<i32>,) = sqlx::query_as(
            "SELECT MAX(seq) FROM chat_messages WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.db)
        .await
        .map_err(|e| StorageError::database(format!("[ChatStore] next seq failed: {}", e)))?;

        Ok(row.0.unwrap_or(0) + 1)
    }

    /// 按会话删除消息。
    pub async fn delete_messages_by_session(&self, session_id: &str) -> Result<u64, StorageError> {
        let deleted = sqlx::query("DELETE FROM chat_messages WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.db)
            .await
            .map_err(|e| StorageError::database(format!("[ChatStore] delete messages failed: {}", e)))?
            .rows_affected();

        Ok(deleted)
    }

    fn row_to_session(row: &PgRow) -> ChatSessionRow {
        ChatSessionRow {
            id: row.get("id"),
            user_id: row.get("user_id"),
            title: row.get("title"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            metadata: row.get("metadata"),
        }
    }

    fn row_to_message(row: &PgRow) -> ChatMessage {
        let tool_calls: Option<serde_json::Value> = row.get("tool_calls");
        let tool_calls = tool_calls.and_then(|v| serde_json::from_value(v).ok());

        ChatMessage {
            id: row.get("id"),
            role: row.get("role"),
            content: row.get("content"),
            timestamp: row.get("timestamp"),
            tool_call_id: row.get("tool_call_id"),
            tool_calls,
            metadata: row.get("metadata"),
        }
    }
}

// ChatMessage 是 services 层的类型，这里做局部解耦用的扩展。
// 由于 sqlx 绑定需要 Option<Vec<ToolCallInfo>> 直接对应 JSONB，
// 我们依赖 serde 对 ToolCallInfo 的 derive Serialize/Deserialize。

