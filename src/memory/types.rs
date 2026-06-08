// src/memory/types.rs
//
// 持久化记忆系统的核心数据结构定义。
//
// 包含：
// - MemoryEntry: 记忆条目的完整数据结构（含向量）
// - VectorRecord: 向量存储记录（序列化格式）
// - SearchResult: 搜索结果
// - MemorySource: 记忆来源分类
// - StoreStats: 存储统计信息

use std::time::{SystemTime, UNIX_EPOCH};

/// 记忆来源分类
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MemorySource {
    /// 对话中的重要用户输入
    UserInput,
    /// 工具执行结果中的关键信息
    ToolOutput,
    /// Agent 的决策/推理
    AgentReasoning,
    /// 系统提取的摘要
    Summary,
    /// 手动保存
    Manual,
    /// 对话自动提取
    Conversation,
}

impl MemorySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemorySource::UserInput => "UserInput",
            MemorySource::ToolOutput => "ToolOutput",
            MemorySource::AgentReasoning => "AgentReasoning",
            MemorySource::Summary => "Summary",
            MemorySource::Manual => "Manual",
            MemorySource::Conversation => "Conversation",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "UserInput" => MemorySource::UserInput,
            "ToolOutput" => MemorySource::ToolOutput,
            "AgentReasoning" => MemorySource::AgentReasoning,
            "Summary" => MemorySource::Summary,
            "Conversation" => MemorySource::Conversation,
            _ => MemorySource::Manual,
        }
    }
}

/// 向量存储记录（序列化格式，用于文件持久化）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VectorRecord {
    pub id: String,
    pub content: String,
    pub vector: Vec<f32>,
    pub tags: Vec<String>,
    pub importance: f32,
    pub source: String,
    pub created_at: String,
    pub accessed_at: String,
    pub access_count: u32,
}

/// 搜索结果
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub record: VectorRecord,
    /// 余弦相似度分数 (0.0 ~ 1.0)
    pub score: f32,
}

/// 存储统计信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreStats {
    pub total_entries: usize,
    pub last_compaction: String,
    pub vector_dim: usize,
}

/// 存储索引（不含向量，用于快速过滤和列表）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: f32,
    pub source: String,
    pub created_at: String,
    pub accessed_at: String,
    pub access_count: u32,
}

impl From<&VectorRecord> for IndexEntry {
    fn from(r: &VectorRecord) -> Self {
        IndexEntry {
            id: r.id.clone(),
            content: r.content.clone(),
            tags: r.tags.clone(),
            importance: r.importance,
            source: r.source.clone(),
            created_at: r.created_at.clone(),
            accessed_at: r.accessed_at.clone(),
            access_count: r.access_count,
        }
    }
}

/// 存储索引文件格式
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreIndex {
    pub entries: Vec<IndexEntry>,
    pub stats: StoreStats,
}

/// 生成当前时间戳字符串
pub fn now_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // Format: 2025-06-14T10:30:00.000Z
    let datetime = chrono_now_from_secs(secs);
    format!("{}.{:03}Z", datetime, millis)
}

fn chrono_now_from_secs(secs: u64) -> String {
    // Simple UTC formatting without chrono crate
    let days_since_epoch = secs / 86400;
    let remaining_secs = secs % 86400;
    let hours = remaining_secs / 3600;
    let minutes = (remaining_secs % 3600) / 60;
    let seconds = remaining_secs % 60;

    // Calculate year/month/day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_date(days_since_epoch as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since unix epoch to year/month/day
fn days_to_date(days: i64) -> (i64, u32, u32) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// 生成唯一 ID
pub fn generate_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("mem_{:x}_{:x}", now.as_nanos(), count)
}

/// 计算余弦相似度
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b + 1e-10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);

        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_generate_id() {
        let id1 = generate_id();
        let id2 = generate_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("mem_"));
    }

    #[test]
    fn test_memory_source_roundtrip() {
        let sources = vec![
            MemorySource::UserInput,
            MemorySource::ToolOutput,
            MemorySource::AgentReasoning,
            MemorySource::Summary,
            MemorySource::Manual,
        ];
        for s in &sources {
            assert_eq!(MemorySource::from_str(s.as_str()), *s);
        }
    }
}
