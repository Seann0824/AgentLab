use std::collections::HashMap;

use chrono::Local;
use serde_json::Value;

use crate::error::AgentLabError;
use crate::services::ServiceError;
use crate::storage::MemoryStore;

use super::base::{MemoryConfig, MemoryItem, RetrieveRequest};
use super::strategy::{MemoryStrategy, StorageScope};

/// 统一记忆引擎。
///
/// 通过 `MemoryStrategy` 把不同记忆类型（working/episodic/semantic/perceptual）
/// 的差异收敛为“策略”：引擎负责统一调度 add / retrieve / forget，
/// 具体怎么存、怎么召回、怎么打分由各策略实现。
pub struct MemoryEngine {
    store: MemoryStore,
    #[allow(dead_code)]
    config: MemoryConfig,
    strategies: HashMap<String, Box<dyn MemoryStrategy>>,
    in_memory: HashMap<String, Vec<MemoryItem>>,
}

impl MemoryEngine {
    pub fn new(
        store: MemoryStore,
        config: MemoryConfig,
        strategies: Vec<Box<dyn MemoryStrategy>>,
    ) -> Self {
        let strategies: HashMap<String, Box<dyn MemoryStrategy>> = strategies
            .into_iter()
            .map(|s| (s.memory_type().to_string(), s))
            .collect();

        Self {
            store,
            config,
            strategies,
            in_memory: HashMap::new(),
        }
    }

    /// 返回已注册的所有记忆类型。
    pub fn memory_types(&self) -> Vec<String> {
        self.strategies.keys().cloned().collect()
    }

    /// 新增记忆。
    ///
    /// 根据策略的 `storage_scope` 决定写入持久化存储还是进程内缓存。
    pub async fn add(&mut self, item: MemoryItem) -> String {
        let memory_type = item.memory_type.clone();
        let strategy = if self.strategies.contains_key(&memory_type) {
            self.strategies.get(&memory_type).unwrap()
        } else {
            self.strategies.get("perceptual").expect("至少应注册 perceptual 策略")
        };

        match strategy.storage_scope() {
            StorageScope::InMemory => {
                self.in_memory
                    .entry(memory_type)
                    .or_default()
                    .push(item.clone());
            }
            _ => {
                if let Err(e) = strategy.enrich_and_store(item.clone(), &mut self.store).await {
                    tracing::error!(
                        "[MemoryEngine] enrich_and_store failed for {}: {}",
                        memory_type,
                        e
                    );
                }
            }
        }

        item.id
    }

    /// 按类型检索记忆。
    pub async fn retrieve_by_type(
        &mut self,
        memory_type: &str,
        request: RetrieveRequest,
    ) -> Vec<MemoryItem> {
        let strategy = match self.strategies.get(memory_type) {
            Some(s) => s,
            None => {
                tracing::warn!("[MemoryEngine] unknown memory type: {}", memory_type);
                return Vec::new();
            }
        };

        let in_memory = self.in_memory.get(memory_type).map(|v| v.as_slice()).unwrap_or(&[]);
        let candidates = strategy
            .retrieve_candidates(&request, &self.store, in_memory)
            .await;

        let now_ts = Local::now().timestamp();
        let importance_threshold = request.importance_threshold.unwrap_or(0.0);
        let limit = request.limit.unwrap_or(5);

        let mut scored: Vec<(f64, MemoryItem)> = candidates
            .into_iter()
            .filter(|(item, _)| {
                // 跳过已标记遗忘的记忆
                !item
                    .metadata
                    .get("forgotten")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    && item.importance >= importance_threshold
            })
            .map(|(mut item, raw_score)| {
                let score = strategy.score(&item, raw_score, now_ts);
                if let Some(obj) = item.metadata.as_object_mut() {
                    obj.insert("relevance_score".to_string(), serde_json::json!(score));
                    obj.insert("memory_type".to_string(), serde_json::json!(memory_type));
                }
                (score, item)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().take(limit).map(|(_, item)| item).collect()
    }

    /// 遗忘指定类型的记忆。
    ///
    /// 支持 `importance_based`、`time_based`、`capacity_based`。
    pub async fn forget(
        &mut self,
        memory_type: &str,
        strategy_name: &str,
        threshold: f64,
        max_age_days: i64,
    ) -> Result<usize, AgentLabError> {
        let strategy = self.strategies.get(memory_type).ok_or_else(|| {
            ServiceError::invalid_argument(format!("记忆类型 {} 不存在", memory_type))
        })?;

        let now_ts = Local::now().timestamp();
        let items = self.list_by_type(memory_type, None, None).await?;

        let to_remove: Vec<String> = if strategy_name == "capacity_based" {
            self.forget_by_capacity(strategy, memory_type, &items, now_ts)
        } else {
            items
                .into_iter()
                .filter(|item| strategy.should_forget(item, strategy_name, threshold, max_age_days, now_ts))
                .map(|item| item.id)
                .collect()
        };

        self.remove_many(memory_type, &to_remove).await?;
        Ok(to_remove.len())
    }

    fn forget_by_capacity(
        &self,
        strategy: &Box<dyn MemoryStrategy>,
        _memory_type: &str,
        items: &[MemoryItem],
        now_ts: i64,
    ) -> Vec<String> {
        let capacity = match strategy.capacity() {
            Some(cap) => cap,
            None => return Vec::new(),
        };

        if items.len() <= capacity {
            return Vec::new();
        }

        let excess = items.len() - capacity;
        let mut indexed: Vec<(f64, &MemoryItem)> = items
            .iter()
            .map(|item| (strategy.score(item, None, now_ts), item))
            .collect();
        indexed.sort_by(|a, b| a.0.total_cmp(&b.0));

        indexed
            .into_iter()
            .take(excess)
            .map(|(_, item)| item.id.clone())
            .collect()
    }

    async fn remove_many(
        &mut self,
        memory_type: &str,
        ids: &[String],
    ) -> Result<(), AgentLabError> {
        let strategy = self.strategies.get(memory_type).ok_or_else(|| {
            ServiceError::invalid_argument(format!("记忆类型 {} 不存在", memory_type))
        })?;

        match strategy.storage_scope() {
            StorageScope::InMemory => {
                if let Some(list) = self.in_memory.get_mut(memory_type) {
                    list.retain(|item| !ids.contains(&item.id));
                }
            }
            _ => {
                for id in ids {
                    let _ = self.store.delete(id).await;
                }
            }
        }

        Ok(())
    }

    /// 更新记忆。
    pub async fn update(
        &self,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f64>,
        metadata: Option<&Value>,
    ) -> Result<bool, AgentLabError> {
        let ok = self
            .store
            .update(memory_id, content, importance, metadata)
            .await?;
        Ok(ok)
    }

    /// 删除单条记忆。
    pub async fn remove(&self, memory_id: &str) -> Result<bool, AgentLabError> {
        // 对于 in-memory 类型，MemoryService 应先通过 list 找到再手动移除；
        // 这里 store.delete 对 in-memory 无效，但也不会报错。
        let ok = self.store.delete(memory_id).await?;
        Ok(ok)
    }

    /// 获取单条记忆。
    pub async fn get(&self, memory_id: &str) -> Result<Option<MemoryItem>, AgentLabError> {
        let item = self.store.get(memory_id).await?;
        Ok(item)
    }

    /// 按类型列出记忆。
    pub async fn list_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<MemoryItem>, AgentLabError> {
        let strategy = self.strategies.get(memory_type).ok_or_else(|| {
            ServiceError::invalid_argument(format!("记忆类型 {} 不存在", memory_type))
        })?;

        match strategy.storage_scope() {
            StorageScope::InMemory => {
                let mut items: Vec<MemoryItem> = self
                    .in_memory
                    .get(memory_type)
                    .map(|list| {
                        list.iter()
                            .filter(|item| {
                                user_id.map(|uid| item.user_id == uid).unwrap_or(true)
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();

                items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                if let Some(l) = limit {
                    items.truncate(l as usize);
                }
                Ok(items)
            }
            _ => {
                let items = self
                    .store
                    .list_by_type(memory_type, user_id, limit)
                    .await?;
                Ok(items)
            }
        }
    }

    /// 按类型统计数量。
    pub async fn count_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<i64, AgentLabError> {
        let strategy = self.strategies.get(memory_type).ok_or_else(|| {
            ServiceError::invalid_argument(format!("记忆类型 {} 不存在", memory_type))
        })?;

        match strategy.storage_scope() {
            StorageScope::InMemory => {
                let count = self
                    .in_memory
                    .get(memory_type)
                    .map(|list| {
                        list.iter()
                            .filter(|item| {
                                user_id.map(|uid| item.user_id == uid).unwrap_or(true)
                            })
                            .count() as i64
                    })
                    .unwrap_or(0);
                Ok(count)
            }
            _ => {
                let count = self.store.count_by_type(memory_type, user_id).await?;
                Ok(count)
            }
        }
    }

    /// 按类型计算平均重要性。
    pub async fn avg_importance_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, AgentLabError> {
        let strategy = self.strategies.get(memory_type).ok_or_else(|| {
            ServiceError::invalid_argument(format!("记忆类型 {} 不存在", memory_type))
        })?;

        match strategy.storage_scope() {
            StorageScope::InMemory => {
                let items: Vec<&MemoryItem> = self
                    .in_memory
                    .get(memory_type)
                    .map(|list| {
                        list.iter()
                            .filter(|item| {
                                user_id.map(|uid| item.user_id == uid).unwrap_or(true)
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                if items.is_empty() {
                    return Ok(None);
                }
                let avg = items.iter().map(|item| item.importance).sum::<f64>() / items.len() as f64;
                Ok(Some(avg))
            }
            _ => {
                let avg = self.store.avg_importance_by_type(memory_type, user_id).await?;
                Ok(avg)
            }
        }
    }

    /// 按类型计算时间跨度（天）。
    pub async fn time_span_days_by_type(
        &self,
        memory_type: &str,
        user_id: Option<&str>,
    ) -> Result<Option<f64>, AgentLabError> {
        let strategy = self.strategies.get(memory_type).ok_or_else(|| {
            ServiceError::invalid_argument(format!("记忆类型 {} 不存在", memory_type))
        })?;

        match strategy.storage_scope() {
            StorageScope::InMemory => {
                let items: Vec<&MemoryItem> = self
                    .in_memory
                    .get(memory_type)
                    .map(|list| {
                        list.iter()
                            .filter(|item| {
                                user_id.map(|uid| item.user_id == uid).unwrap_or(true)
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                if items.len() < 2 {
                    return Ok(None);
                }
                let min_ts = items.iter().map(|item| item.timestamp).min().unwrap_or(0);
                let max_ts = items.iter().map(|item| item.timestamp).max().unwrap_or(0);
                let span_days = (max_ts - min_ts) as f64 / 86400.0;
                Ok(Some(span_days))
            }
            _ => {
                let span = self
                    .store
                    .time_span_days_by_type(memory_type, user_id)
                    .await?;
                Ok(span)
            }
        }
    }

    /// 清空指定类型的记忆。
    pub async fn clear_by_type(&mut self, memory_type: &str) -> Result<u64, AgentLabError> {
        let strategy = self.strategies.get(memory_type).ok_or_else(|| {
            ServiceError::invalid_argument(format!("记忆类型 {} 不存在", memory_type))
        })?;

        match strategy.storage_scope() {
            StorageScope::InMemory => {
                let count = self
                    .in_memory
                    .get(memory_type)
                    .map(|list| list.len() as u64)
                    .unwrap_or(0);
                self.in_memory.remove(memory_type);
                Ok(count)
            }
            _ => {
                let count = self.store.clear_by_type(memory_type).await?;
                Ok(count)
            }
        }
    }

}
