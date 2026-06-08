// src/memory/store.rs
//
// VectorStore — 本地文件向量存储 + 余弦相似度搜索。
//
// 存储结构：
//   .memory/
//   ├── index.json     # 记忆索引（元数据，不含向量）
//   └── vectors.bin    # 向量数据（二进制，按 ID 顺序存储）
//
// 搜索时：加载全量向量到内存做余弦相似度搜索。
// 对于 < 10000 条记录，线性扫描足够快。
//
// 设计原则：
// - 无需外部依赖
// - 文件级持久化，进程重启不丢失
// - 简单的线性搜索，适合中小规模

use std::collections::HashMap;
use std::path::PathBuf;

use super::types::*;

/// 向量存储
pub struct VectorStore {
    store_dir: PathBuf,
    /// 内存中的向量数据 id -> vector
    vectors: HashMap<String, Vec<f32>>,
    /// 存储索引（元数据）
    index: StoreIndex,
    /// 脏标记：是否有未写入的变更
    dirty: bool,
}

impl VectorStore {
    /// 打开或创建向量存储
    pub fn open(store_dir: PathBuf, vector_dim: usize) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&store_dir)?;

        let index_path = store_dir.join("index.json");
        let index = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path)?;
            serde_json::from_str::<StoreIndex>(&content).unwrap_or(StoreIndex {
                entries: Vec::new(),
                stats: StoreStats {
                    total_entries: 0,
                    last_compaction: now_timestamp(),
                    vector_dim,
                },
            })
        } else {
            StoreIndex {
                entries: Vec::new(),
                stats: StoreStats {
                    total_entries: 0,
                    last_compaction: now_timestamp(),
                    vector_dim,
                },
            }
        };

        let mut store = Self {
            store_dir,
            vectors: HashMap::new(),
            index,
            dirty: false,
        };

        // Load vectors from binary file
        store.load_vectors()?;

        Ok(store)
    }

    /// 加载向量数据
    fn load_vectors(&mut self) -> anyhow::Result<()> {
        let vectors_path = self.store_dir.join("vectors.bin");
        if !vectors_path.exists() {
            return Ok(());
        }

        let data = std::fs::read(&vectors_path)?;
        let dim = self.index.stats.vector_dim;
        let record_size = dim * 4; // f32 = 4 bytes
        let expected_count = data.len() / record_size;

        self.vectors.clear();
        for (i, entry) in self.index.entries.iter().enumerate() {
            if i < expected_count {
                let start = i * record_size;
                let end = start + record_size;
                if end <= data.len() {
                    let mut vec = Vec::with_capacity(dim);
                    for j in 0..dim {
                        let offset = start + j * 4;
                        if offset + 4 <= data.len() {
                            let bytes: [u8; 4] = [
                                data[offset],
                                data[offset + 1],
                                data[offset + 2],
                                data[offset + 3],
                            ];
                            vec.push(f32::from_le_bytes(bytes));
                        }
                    }
                    self.vectors.insert(entry.id.clone(), vec);
                }
            }
        }

        Ok(())
    }

    /// 保存索引和向量到文件
    pub fn flush(&mut self) -> anyhow::Result<()> {
        if !self.dirty {
            return Ok(());
        }

        // Save index
        self.index.stats.total_entries = self.index.entries.len();
        let index_path = self.store_dir.join("index.json");
        let content = serde_json::to_string_pretty(&self.index)?;
        std::fs::write(&index_path, content)?;

        // Save vectors (binary, in index order)
        let dim = self.index.stats.vector_dim;
        let mut vector_data = Vec::new();
        for entry in &self.index.entries {
            if let Some(vec) = self.vectors.get(&entry.id) {
                for &v in vec {
                    vector_data.extend_from_slice(&v.to_le_bytes());
                }
            } else {
                // Fill with zeros if vector not found
                for _ in 0..dim {
                    vector_data.extend_from_slice(&0f32.to_le_bytes());
                }
            }
        }

        let vectors_path = self.store_dir.join("vectors.bin");
        std::fs::write(&vectors_path, vector_data)?;

        self.dirty = false;
        Ok(())
    }

    /// 插入一条向量记录
    pub fn insert(&mut self, record: VectorRecord) -> anyhow::Result<()> {
        let id = record.id.clone();
        let dim = self.index.stats.vector_dim;

        // Validate vector dimension
        if record.vector.len() != dim {
            return Err(anyhow::anyhow!(
                "Vector dimension mismatch: expected {}, got {}",
                dim,
                record.vector.len()
            ));
        }

        // Check if already exists, update in place
        if let Some(pos) = self.index.entries.iter().position(|e| e.id == id) {
            self.index.entries[pos] = IndexEntry::from(&record);
        } else {
            self.index.entries.push(IndexEntry::from(&record));
        }

        self.vectors.insert(id, record.vector);
        self.dirty = true;

        Ok(())
    }

    /// 搜索最相似的 K 条记录
    pub fn search(&self, query_vector: &[f32], top_k: usize) -> Vec<SearchResult> {
        if self.vectors.is_empty() {
            return Vec::new();
        }

        let mut results: Vec<SearchResult> = self
            .index
            .entries
            .iter()
            .filter_map(|entry| {
                self.vectors
                    .get(&entry.id)
                    .map(|vec| {
                        let score = cosine_similarity(query_vector, vec);
                        SearchResult {
                            record: VectorRecord {
                                id: entry.id.clone(),
                                content: entry.content.clone(),
                                vector: vec.clone(),
                                tags: entry.tags.clone(),
                                importance: entry.importance,
                                source: entry.source.clone(),
                                created_at: entry.created_at.clone(),
                                accessed_at: entry.accessed_at.clone(),
                                access_count: entry.access_count,
                            },
                            score,
                        }
                    })
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Apply importance weighting for final score: similarity * 0.7 + importance * 0.3
        for r in &mut results {
            let importance_factor = r.record.importance;
            r.score = r.score * 0.7 + importance_factor * 0.3;
        }

        // Re-sort after importance weighting
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        results.truncate(top_k);
        results
    }

    /// 按标签过滤 + 语义搜索
    pub fn search_with_tags(
        &self,
        query_vector: &[f32],
        tags: &[String],
        top_k: usize,
    ) -> Vec<SearchResult> {
        if tags.is_empty() {
            return self.search(query_vector, top_k);
        }

        // First filter by tags, then search
        let tag_set: std::collections::HashSet<&str> =
            tags.iter().map(|t| t.as_str()).collect();

        let mut results: Vec<SearchResult> = self
            .index
            .entries
            .iter()
            .filter(|entry| {
                entry
                    .tags
                    .iter()
                    .any(|t| tag_set.contains(t.as_str()))
            })
            .filter_map(|entry| {
                self.vectors
                    .get(&entry.id)
                    .map(|vec| {
                        let score = cosine_similarity(query_vector, vec);
                        SearchResult {
                            record: VectorRecord {
                                id: entry.id.clone(),
                                content: entry.content.clone(),
                                vector: vec.clone(),
                                tags: entry.tags.clone(),
                                importance: entry.importance,
                                source: entry.source.clone(),
                                created_at: entry.created_at.clone(),
                                accessed_at: entry.accessed_at.clone(),
                                access_count: entry.access_count,
                            },
                            score,
                        }
                    })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// 按 ID 删除
    pub fn delete(&mut self, id: &str) -> bool {
        if let Some(pos) = self.index.entries.iter().position(|e| e.id == id) {
            self.index.entries.remove(pos);
            self.vectors.remove(id);
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// 更新重要性评分
    pub fn update_importance(&mut self, id: &str, importance: f32) -> anyhow::Result<()> {
        if let Some(entry) = self.index.entries.iter_mut().find(|e| e.id == id) {
            entry.importance = importance.clamp(0.0, 1.0);
            self.dirty = true;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Memory entry not found: {}", id))
        }
    }

    /// 获取存储统计信息
    pub fn stats(&self) -> StoreStats {
        self.index.stats.clone()
    }

    /// 压缩存储（剔除低重要性条目）
    pub fn compact(&mut self, min_importance: f32) -> usize {
        let before = self.index.entries.len();
        let ids_to_remove: Vec<String> = self
            .index
            .entries
            .iter()
            .filter(|e| e.importance < min_importance && e.access_count < 2)
            .map(|e| e.id.clone())
            .collect();

        for id in &ids_to_remove {
            self.vectors.remove(id);
        }

        self.index
            .entries
            .retain(|e| e.importance >= min_importance || e.access_count >= 2);

        let removed = before - self.index.entries.len();
        if removed > 0 {
            self.dirty = true;
            self.index.stats.last_compaction = now_timestamp();
        }

        removed
    }

    /// 更新访问计数
    pub fn record_access(&mut self, id: &str) {
        if let Some(entry) = self.index.entries.iter_mut().find(|e| e.id == id) {
            entry.access_count += 1;
            entry.accessed_at = now_timestamp();
            self.dirty = true;
        }
    }

    /// 列出所有条目（按重要性降序）
    pub fn list_entries(&self, limit: usize) -> Vec<IndexEntry> {
        let mut entries = self.index.entries.clone();
        entries.sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap_or(std::cmp::Ordering::Equal));
        entries.truncate(limit);
        entries
    }
}

impl Drop for VectorStore {
    fn drop(&mut self) {
        if self.dirty {
            let _ = self.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_store() -> (VectorStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("memory_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = VectorStore::open(dir.clone(), 4).unwrap();
        (store, dir)
    }

    fn make_record(id: &str, content: &str, vector: Vec<f32>, importance: f32) -> VectorRecord {
        VectorRecord {
            id: id.to_string(),
            content: content.to_string(),
            vector,
            tags: vec!["test".to_string()],
            importance,
            source: "Manual".to_string(),
            created_at: now_timestamp(),
            accessed_at: now_timestamp(),
            access_count: 0,
        }
    }

    #[test]
    fn test_insert_and_search() {
        let (mut store, dir) = create_test_store();

        store.insert(make_record("1", "rust programming", vec![1.0, 0.0, 0.0, 0.0], 0.8)).unwrap();
        store.insert(make_record("2", "python data science", vec![0.0, 1.0, 0.0, 0.0], 0.6)).unwrap();
        store.insert(make_record("3", "javascript web", vec![0.0, 0.0, 1.0, 0.0], 0.5)).unwrap();

        // Search for rust-related
        let results = store.search(&[1.0, 0.0, 0.0, 0.0], 2);
        assert!(!results.is_empty());
        assert_eq!(results[0].record.id, "1");

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_delete() {
        let (mut store, dir) = create_test_store();
        store.insert(make_record("1", "test", vec![1.0, 0.0, 0.0, 0.0], 0.5)).unwrap();
        assert!(store.delete("1"));
        assert!(!store.delete("1"));
        assert!(store.search(&[1.0, 0.0, 0.0, 0.0], 10).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_compact() {
        let (mut store, dir) = create_test_store();
        store.insert(make_record("1", "important", vec![1.0, 0.0, 0.0, 0.0], 0.8)).unwrap();
        store.insert(make_record("2", "unimportant", vec![0.0, 1.0, 0.0, 0.0], 0.2)).unwrap();

        let removed = store.compact(0.5);
        assert_eq!(removed, 1);

        let entries = store.list_entries(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_persistence() {
        let dir = std::env::temp_dir().join(format!("memory_persist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // Write
        {
            let mut store = VectorStore::open(dir.clone(), 4).unwrap();
            store.insert(make_record("1", "persistent data", vec![1.0, 0.0, 0.0, 0.0], 0.7)).unwrap();
            store.flush().unwrap();
        }

        // Read back
        {
            let store = VectorStore::open(dir.clone(), 4).unwrap();
            let results = store.search(&[1.0, 0.0, 0.0, 0.0], 10);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].record.content, "persistent data");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
