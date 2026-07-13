use crate::services::ServiceError;
use crate::services::chat_dto::ChatMessage;
use crate::storage::ChatStore;

/// 消息业务服务：负责会话消息的 CRUD。
#[derive(Clone)]
pub struct MessageService {
    store: ChatStore,
}

impl MessageService {
    pub fn new(store: ChatStore) -> Self {
        Self { store }
    }

    /// 添加消息，seq 由存储层自动分配。
    pub async fn add(&self, session_id: &str, message: &ChatMessage) -> Result<(), ServiceError> {
        let seq = self.store.next_seq(session_id).await?;
        self.store.add_message(session_id, message, seq).await?;
        Ok(())
    }

    /// 获取会话完整消息历史。
    pub async fn history(&self, session_id: &str) -> Result<Vec<ChatMessage>, ServiceError> {
        Ok(self.store.get_messages(session_id).await?)
    }

    /// 删除会话下所有消息。
    pub async fn clear(&self, session_id: &str) -> Result<u64, ServiceError> {
        Ok(self.store.delete_messages_by_session(session_id).await?)
    }
}
