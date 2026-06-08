// src/memory/manager.rs
//
// MemoryManager — 记忆系统的对外统一接口。
//
// 职责：
// 1. 协调 EmbeddingClient 和 VectorStore
// 2. 提供记忆 CRUD 操作
// 3. 提供上下文注入文本生成
// 4. 管理记忆生命周期（自动提取、遗忘）

use std::path::PathBuf;

use super::embedding::EmbeddingClient;
use super::store::VectorStore;
use super::types::*;

/// 记忆管理器 — 所有记忆操作的入口
pub struct MemoryManager {
    /// Embedding 客户端
    embedding: Option<EmbeddingClient>,
    /// 向量存储
    store: VectorStore,
    /// 是否启用
    enabled: bool,
    /// 相似度阈值
    similarity_threshold: f32,
    /// 最大条目数
    max_entries: usize,
}

impl MemoryManager {
    /// 创建新的记忆管理器
    ///
    /// 自动尝试从环境变量初始化 EmbeddingClient。
    /// 如果环境变量未配置，EmbeddingClient 为 None（降级为纯标签搜索）。
    pub async fn new(store_dir: PathBuf) -> anyhow::Result<Self> {
        let embedding = EmbeddingClient::from_env().ok();
        let vector_dim = embedding.as_ref().map(|e| e.vector_dim()).unwrap_or(4);

        let store = VectorStore::open(store_dir, vector_dim)?;

        let max_entries = std::env::var("MEMORY_MAX_ENTRIES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1000);

        let similarity_threshold = std::env::var("MEMORY_SIMILARITY_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.6);

        Ok(Self {
            embedding,
            store,
            enabled: true,
            similarity_threshold,
            max_entries,
        })
    }

    /// 创建一个测试用的 MemoryManager（使用模拟向量，不依赖外部 API）
    pub fn new_mock(store_dir: PathBuf) -> Self {
        let store = VectorStore::open(store_dir.clone(), 4).unwrap_or_else(|_| {
            VectorStore::open(store_dir, 4).expect("Failed to create store")
        });

        Self {
            embedding: None,
            store,
            enabled: true,
            similarity_threshold: 0.6,
            max_entries: 1000,
        }
    }

    /// 保存一条记忆（自动生成向量）
    pub async fn save(
        &mut self,
        content: &str,
        tags: &[String],
        source: MemorySource,
        importance: f32,
    ) -> anyhow::Result<String> {
        if !self.enabled {
            return Err(anyhow::anyhow!("Memory system is disabled"));
        }

        // Check max entries, compact if needed
        if self.store.stats().total_entries >= self.max_entries {
            self.store.compact(0.3);
        }

        // Generate vector
        let vector = if let Some(emb) = &self.embedding {
            emb.embed(content).await.unwrap_or_else(|_| {
                // Fallback: use a zero vector if embedding fails
                vec![0.0; emb.vector_dim()]
            })
        } else {
            // Mock vector for testing without API
            let dim = self.store.stats().vector_dim;
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            content.hash(&mut hasher);
            let hash = hasher.finish();
            (0..dim).map(|i| ((hash >> (i % 8) * 8) & 0xFF) as f32 / 255.0).collect()
        };

        let id = generate_id();
        let now = now_timestamp();

        let record = VectorRecord {
            id: id.clone(),
            content: content.to_string(),
            vector,
            tags: tags.to_vec(),
            importance: importance.clamp(0.0, 1.0),
            source: source.as_str().to_string(),
            created_at: now.clone(),
            accessed_at: now,
            access_count: 0,
        };

        self.store.insert(record)?;
        self.store.flush()?;

        Ok(id)
    }

    /// 搜索相关记忆（使用文本查询，自动向量化）
    pub async fn search_similar(&self, text: &str, top_k: usize) -> anyhow::Result<Vec<SearchResult>> {
        if !self.enabled {
            return Ok(Vec::new());
        }

        let query_vector = if let Some(emb) = &self.embedding {
            emb.embed(text).await?
        } else {
            // Mock vector for testing without API
            let dim = self.store.stats().vector_dim;
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            let hash = hasher.finish();
            (0..dim).map(|i| ((hash >> (i % 8) * 8) & 0xFF) as f32 / 255.0).collect()
        };

        let mut results = self.store.search(&query_vector, top_k);

        // Apply similarity threshold filter
        results.retain(|r| r.score >= self.similarity_threshold);

        Ok(results)
    }

    /// 获取与当前上下文相关的记忆（用于注入）
    pub async fn get_relevant_memories(&self, contexts: &[&str], top_k: usize) -> anyhow::Result<Vec<SearchResult>> {
        if !self.enabled || contexts.is_empty() {
            return Ok(Vec::new());
        }

        // Use the most recent context as query
        let query = contexts.last().unwrap_or(&"");
        self.search_similar(query, top_k).await
    }

    /// 获取记忆注入文本（用于系统提示词）
    pub async fn get_injection_text(&self, contexts: &[&str]) -> anyhow::Result<String> {
        if !self.enabled {
            return Ok(String::new());
        }

        let memories = self.get_relevant_memories(contexts, 5).await?;
        if memories.is_empty() {
            return Ok(String::new());
        }

        let mut text = String::from("\n\n[相关记忆 — 跨会话长期记忆]\n以下是你之前记住的重要信息（按相关度排序）：\n\n");
        for (i, mem) in memories.iter().enumerate() {
            text.push_str(&format!(
                "{}. {} (来源: {}, 重要性: {:.2})\n",
                i + 1,
                mem.record.content,
                mem.record.source,
                mem.record.importance,
            ));
        }

        Ok(text)
    }

    /// 删除一条记忆
    pub fn forget(&mut self, id: &str) -> bool {
        self.store.delete(id)
    }

    /// 列出记忆（按重要性排序）
    pub fn list(&self, limit: usize) -> Vec<IndexEntry> {
        self.store.list_entries(limit)
    }

    /// 获取统计信息
    pub fn stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// 开启/关闭记忆系统
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// 检查是否启用
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 获取配置信息
    pub fn config_info(&self) -> String {
        let embed_info = match &self.embedding {
            Some(emb) => format!("model={}, dim={}", emb.model_name(), emb.vector_dim()),
            None => "not configured (using mock vectors)".to_string(),
        };
        format!(
            "MemoryManager: enabled={}, embedding=[{}], entries={}, threshold={}, max={}",
            self.enabled,
            embed_info,
            self.store.stats().total_entries,
            self.similarity_threshold,
            self.max_entries,
        )
    }

    /// 手动刷新存储
    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.store.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_manager() -> (MemoryManager, PathBuf) {
        let dir = std::env::temp_dir().join(format!("memory_mgr_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mgr = MemoryManager::new_mock(dir.clone());
        (mgr, dir)
    }

    #[tokio::test]
    async fn test_save_and_search() {
        let (mut mgr, dir) = create_test_manager();

        let id = mgr.save(
            "Users prefer Python for data science",
            &["user-preference".to_string(), "python".to_string()],
            MemorySource::UserInput,
            0.8,
        ).await.unwrap();
        assert!(id.starts_with("mem_"));

        mgr.save(
            "Project uses PostgreSQL database",
            &["decision".to_string(), "database".to_string()],
            MemorySource::AgentReasoning,
            0.7,
        ).await.unwrap();

        // Search for related memory
        let results = mgr.search_similar("what database", 5).await.unwrap();
        assert!(!results.is_empty(), "Should find at least one result");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_forget() {
        let (mut mgr, dir) = create_test_manager();
        let id = mgr.save("test memory", &[], MemorySource::Manual, 0.5).await.unwrap();
        assert!(mgr.forget(&id));
        assert!(!mgr.forget(&id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list() {
        let (mgr, dir) = create_test_manager();
        let entries = mgr.list(10);
        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_info() {
        let (mgr, dir) = create_test_manager();
        let info = mgr.config_info();
        assert!(info.contains("MemoryManager"));
        assert!(info.contains("enabled=true"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
