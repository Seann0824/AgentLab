use super::super::types::chrono_now;

/// Goal 索引条目（轻量摘要）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalIndexEntry {
    pub id: String,
    pub name: String,
    pub status: String,
    pub progress: u8,
    pub updated_at: String,
}

/// Goal 索引文件结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct GoalIndex {
    pub(super) goals: Vec<GoalIndexEntry>,
    pub(super) last_updated: String,
}

impl GoalIndex {
    pub(super) fn new(goals: Vec<GoalIndexEntry>) -> Self {
        Self {
            goals,
            last_updated: chrono_now(),
        }
    }
}
