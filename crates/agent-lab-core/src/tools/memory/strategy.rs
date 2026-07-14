use crate::error::AgentLabError;

use super::base::{ConflictResolution, MemoryItem, RetrieveRequest};
use crate::storage::MemoryStore;

/// 记忆存储范围。
///
/// 不同记忆类型对持久化的要求不同：有的只保留在进程内，有的需要写入 PG，
/// 有的还需要维护 Neo4j 实体关系图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageScope {
    /// 仅进程内存储（working / perceptual）
    InMemory,
    /// 持久化到 PostgreSQL（episodic）
    Persistent,
    /// 持久化到 PostgreSQL + Neo4j 引用图（semantic）
    PersistentWithGraph,
}

/// 记忆策略：定义某一类记忆的存储、召回、评分与遗忘方式。
///
/// `MemoryEngine` 持有多个策略，统一调用它们完成 add / retrieve / forget，
/// 避免每种记忆类型都实现一整个 `Memory` trait。
#[async_trait::async_trait]
pub trait MemoryStrategy: Send + Sync {
    /// 策略对应的记忆类型名。
    fn memory_type(&self) -> &'static str;

    /// 该策略的存储范围。
    fn storage_scope(&self) -> StorageScope;

    /// 新增记忆时的附加处理。
    ///
    /// 例如语义记忆会在这里抽取实体/关系并写入 Neo4j；
    /// 非持久化策略可空实现。
    async fn enrich_and_store(
        &self,
        item: MemoryItem,
        store: &mut MemoryStore,
    ) -> Result<(), AgentLabError>;

    /// 根据请求召回候选记忆。
    ///
    /// 返回每个候选及其原始分数（如向量距离、关键词重叠度等），
    /// 供 `score` 做进一步计算。
    async fn retrieve_candidates(
        &self,
        request: &RetrieveRequest,
        store: &MemoryStore,
        in_memory: &[MemoryItem],
    ) -> Vec<(MemoryItem, Option<f64>)>;

    /// 对召回候选打分。
    ///
    /// `raw_score` 是 `retrieve_candidates` 返回的原始分数，不同策略含义不同：
    /// - 向量策略：cosine similarity
    /// - 关键词策略：重叠度
    /// - TF-IDF 策略：cosine similarity
    fn score(&self, item: &MemoryItem, raw_score: Option<f64>, now_ts: i64) -> f64;

    /// 判断给定记忆是否应被遗忘。
    fn should_forget(
        &self,
        item: &MemoryItem,
        strategy: &str,
        threshold: f64,
        max_age_days: i64,
        now_ts: i64,
    ) -> bool;

    /// 该策略的容量上限。
    ///
    /// 返回 `None` 表示不限制。用于 `capacity_based` 遗忘策略。
    fn capacity(&self) -> Option<usize> {
        None
    }

    /// 该策略是否支持冲突裁决（查重 / 变更 / 失效）。
    ///
    /// 默认返回 false，保持现有策略行为不变。
    fn supports_conflict_resolution(&self) -> bool {
        false
    }

    /// 在新增记忆前进行冲突裁决。
    ///
    /// 返回 `ConflictResolution` 告诉引擎：
    /// - 新事实是直接新增、跳过，还是合并到已有记忆；
    /// - 每条候选已有记忆应该保留、更新、失效还是删除。
    ///
    /// 默认实现直接返回 `Add`，即不处理冲突。
    async fn resolve_conflicts(
        &self,
        _new_item: &MemoryItem,
        _store: &MemoryStore,
    ) -> Result<ConflictResolution, AgentLabError> {
        Ok(ConflictResolution::add_new())
    }

    /// 批量冲突裁决。
    ///
    /// 默认实现逐条调用 `resolve_conflicts`；支持批量 LLM 的策略应覆盖此方法。
    async fn resolve_conflicts_batch(
        &self,
        items: &[MemoryItem],
        store: &MemoryStore,
    ) -> Result<Vec<ConflictResolution>, AgentLabError> {
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            results.push(self.resolve_conflicts(item, store).await?);
        }
        Ok(results)
    }
}
