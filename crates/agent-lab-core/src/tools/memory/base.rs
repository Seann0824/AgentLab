use chrono::Local;

use crate::error::AgentLabError;

#[derive(Clone, sqlx::FromRow)]
pub struct MemoryItem {
    pub id: String,
    pub user_id: String,
    pub memory_type: String,
    pub content: String,
    pub timestamp: i64,
    pub importance: f64,
    pub session_id: Option<String>,
    pub metadata: serde_json::Value,
}

impl MemoryItem {
    pub fn new(
        user_id: String,
        memory_type: String,
        content: String,
        importance: f64,
        mut metadata: serde_json::Value,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        if let Some(obj) = metadata.as_object_mut() {
            obj.entry("status".to_string())
                .or_insert_with(|| serde_json::json!("active"));
        }
        Self {
            id,
            user_id,
            memory_type,
            content,
            session_id: Some("default_session".into()), // todo: 目前先设置成默认session，等后续多session场景在拓展。
            timestamp: Local::now().timestamp(),
            importance,
            metadata,
        }
    }

    /// 记忆当前是否处于有效状态。
    pub fn is_active(&self) -> bool {
        self.metadata_status() == "active"
    }

    /// 记忆是否已被标记为失效。
    pub fn is_invalidated(&self) -> bool {
        self.metadata_status() == "invalidated"
    }

    /// 记忆是否已被合并到其他记忆。
    pub fn is_merged(&self) -> bool {
        self.metadata_status() == "merged"
    }

    /// 记忆是否仍可用于检索/召回。
    pub fn is_retrievable(&self) -> bool {
        self.is_active()
    }

    fn metadata_status(&self) -> String {
        self.metadata
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("active")
            .to_string()
    }

    /// 将该记忆标记为被新记忆失效。
    pub fn mark_invalidated(&mut self, invalidated_by: &str, reason: &str) {
        if let Some(obj) = self.metadata.as_object_mut() {
            obj.insert("status".to_string(), serde_json::json!("invalidated"));
            obj.insert(
                "invalidated_by".to_string(),
                serde_json::json!(invalidated_by),
            );
            obj.insert(
                "invalidation_reason".to_string(),
                serde_json::json!(reason),
            );
            obj.insert(
                "invalidated_at".to_string(),
                serde_json::json!(Local::now().timestamp()),
            );
        }
    }

    /// 将该记忆标记为已合并到目标记忆。
    pub fn mark_merged(&mut self, merged_into: &str) {
        if let Some(obj) = self.metadata.as_object_mut() {
            obj.insert("status".to_string(), serde_json::json!("merged"));
            obj.insert(
                "merged_into".to_string(),
                serde_json::json!(merged_into),
            );
            obj.insert(
                "merged_at".to_string(),
                serde_json::json!(Local::now().timestamp()),
            );
        }
    }

    /// 更新记忆内容时同步记录更新时间。
    pub fn mark_updated(&mut self) {
        if let Some(obj) = self.metadata.as_object_mut() {
            obj.insert(
                "updated_at".to_string(),
                serde_json::json!(Local::now().timestamp()),
            );
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct RetrieveRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub time_range: Option<(i64, i64)>,
    pub importance_threshold: Option<f64>,
}

/// 冲突裁决结果：策略根据该结果决定是新增、跳过，还是修改已有记忆。
#[derive(Clone, Debug)]
pub struct ConflictResolution {
    pub new_fact_action: NewFactAction,
    pub existing_memories: Vec<ExistingMemoryDecision>,
}

impl ConflictResolution {
    /// 默认策略：不做冲突处理，直接新增。
    pub fn add_new() -> Self {
        Self {
            new_fact_action: NewFactAction::Add,
            existing_memories: Vec::new(),
        }
    }

    /// 判定为重复：跳过新增，保持已有记忆不变。
    pub fn duplicate(merged_into: String) -> Self {
        Self {
            new_fact_action: NewFactAction::Skip {
                merged_into: Some(merged_into),
            },
            existing_memories: Vec::new(),
        }
    }
}

/// 对新增事实的裁决动作。
#[derive(Clone, Debug)]
pub enum NewFactAction {
    /// 作为新记忆写入。
    Add,
    /// 不写入，因为它与已有记忆重复或已被合并。
    Skip { merged_into: Option<String> },
}

/// 对已有记忆的裁决动作。
#[derive(Clone, Debug)]
pub enum ExistingAction {
    /// 保持原样。
    Keep,
    /// 用合并后的内容更新。
    Update,
    /// 标记为失效（软删除，保留审计）。
    Invalidate,
    /// 物理删除（仅在明确配置下使用）。
    Delete,
}

/// 针对单条已有记忆的裁决决定。
#[derive(Clone, Debug)]
pub struct ExistingMemoryDecision {
    pub memory_id: String,
    pub action: ExistingAction,
    pub merged_content: Option<String>,
    pub reason: String,
}

/// 单条事实写入记忆后的结果动作。
#[derive(Clone, Debug)]
pub enum MemoryWriteAction {
    /// 作为新记忆写入。
    Added,
    /// 与已有记忆重复，未写入。
    SkippedDuplicate,
    /// 合并/更新到已有记忆。
    Merged,
    /// 新增并导致若干旧记忆失效。
    InvalidatedOthers,
}

/// 一条事实经冲突裁决与持久化后的结果。
#[derive(Clone, Debug)]
pub struct MemoryWriteResult {
    pub fact: String,
    pub memory_id: String,
    pub action: MemoryWriteAction,
    /// 当 action 为 InvalidatedOthers 时，记录被失效的旧记忆 ID。
    pub invalidated_ids: Vec<String>,
}

impl MemoryWriteResult {
    pub fn added(fact: impl Into<String>, memory_id: impl Into<String>) -> Self {
        Self {
            fact: fact.into(),
            memory_id: memory_id.into(),
            action: MemoryWriteAction::Added,
            invalidated_ids: Vec::new(),
        }
    }

    pub fn skipped_duplicate(
        fact: impl Into<String>,
        memory_id: impl Into<String>,
    ) -> Self {
        Self {
            fact: fact.into(),
            memory_id: memory_id.into(),
            action: MemoryWriteAction::SkippedDuplicate,
            invalidated_ids: Vec::new(),
        }
    }

    pub fn merged(fact: impl Into<String>, memory_id: impl Into<String>) -> Self {
        Self {
            fact: fact.into(),
            memory_id: memory_id.into(),
            action: MemoryWriteAction::Merged,
            invalidated_ids: Vec::new(),
        }
    }

    pub fn invalidated_others(
        fact: impl Into<String>,
        memory_id: impl Into<String>,
        invalidated_ids: Vec<String>,
    ) -> Self {
        Self {
            fact: fact.into(),
            memory_id: memory_id.into(),
            action: MemoryWriteAction::InvalidatedOthers,
            invalidated_ids,
        }
    }
}

#[async_trait::async_trait]
pub trait Memory: Send + Sync {
    async fn add(&mut self, memory_item: MemoryItem) -> String;
    async fn retrieve(&mut self, request: RetrieveRequest) -> Vec<MemoryItem>;

    /// 遗忘策略入口。默认不实现，返回 0。
    async fn forget(
        &self,
        _strategy: &str,
        _threshold: f64,
        _max_age_days: i64,
    ) -> Result<usize, AgentLabError> {
        Ok(0)
    }
}

#[derive(Clone)]
pub struct MemoryConfig {
    pub working_memory_capacoty: Option<usize>,
    pub max_age_minutes: Option<i64>,
    pub time_factor: f64,
}

impl MemoryConfig {
    pub fn new() -> Self {
        Self {
            working_memory_capacoty: None,
            max_age_minutes: None,
            time_factor: 0.1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_item_default_status_active() {
        let item = MemoryItem::new(
            "u1".into(),
            "semantic".into(),
            "test content".into(),
            0.5,
            serde_json::json!({}),
        );
        assert!(item.is_active());
        assert!(!item.is_invalidated());
        assert!(!item.is_merged());
        assert!(item.is_retrievable());
    }

    #[test]
    fn test_memory_item_mark_invalidated() {
        let mut item = MemoryItem::new(
            "u1".into(),
            "semantic".into(),
            "old name".into(),
            0.5,
            serde_json::json!({}),
        );
        item.mark_invalidated("new_id", "name changed");
        assert!(!item.is_active());
        assert!(item.is_invalidated());
        assert!(!item.is_retrievable());
        assert_eq!(
            item.metadata.get("invalidated_by").unwrap().as_str(),
            Some("new_id")
        );
    }

    #[test]
    fn test_memory_item_mark_merged() {
        let mut item = MemoryItem::new(
            "u1".into(),
            "semantic".into(),
            "fact".into(),
            0.5,
            serde_json::json!({}),
        );
        item.mark_merged("target_id");
        assert!(item.is_merged());
        assert!(!item.is_retrievable());
    }
}

