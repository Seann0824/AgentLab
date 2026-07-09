use chrono::Local;

use crate::services::chat_dto::SessionSummary;
use crate::services::ServiceError;
use crate::storage::{ChatSessionRow, ChatStore};

/// 会话管理服务：负责会话元数据的 CRUD。
#[derive(Clone)]
pub struct SessionService {
    store: ChatStore,
}

impl SessionService {
    pub fn new(store: ChatStore) -> Self {
        Self { store }
    }

    /// 创建新会话。
    pub async fn create(&self, user_id: &str, session_id: &str) -> Result<(), ServiceError> {
        let now = Local::now().timestamp();
        self.store
            .create_session(session_id, user_id, None, now, now)
            .await?;
        Ok(())
    }

    /// 列出用户会话摘要。
    pub async fn list(&self, user_id: &str) -> Result<Vec<SessionSummary>, ServiceError> {
        let mut summaries = self.store.list_sessions(user_id, 1_000).await?;
        // 如果会话没有标题，从消息里取第一条用户消息作为标题（由调用方决定，这里保持简洁）。
        for summary in &mut summaries {
            if summary.title == "新会话" {
                // ChatService 在加载时会补充标题，这里不额外查询消息。
            }
        }
        Ok(summaries)
    }

    /// 查询会话元数据。
    pub async fn get(&self, session_id: &str) -> Result<Option<ChatSessionRow>, ServiceError> {
        Ok(self.store.get_session(session_id).await?)
    }

    /// 更新会话标题。
    pub async fn rename(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<bool, ServiceError> {
        let now = Local::now().timestamp();
        Ok(self.store.update_session_title(session_id, title, now).await?)
    }

    /// 更新会话最后活动时间。
    pub async fn touch(&self, session_id: &str) -> Result<bool, ServiceError> {
        let now = Local::now().timestamp();
        Ok(self.store.touch_session(session_id, now).await?)
    }

    /// 删除会话（级联删除消息）。
    pub async fn delete(&self, session_id: &str) -> Result<bool, ServiceError> {
        Ok(self.store.delete_session(session_id).await?)
    }
}
