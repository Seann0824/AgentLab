use std::{collections::{BinaryHeap, HashMap, HashSet}, hash::Hash, sync::atomic::AtomicU64, vec};
use chrono::Local;
use scirs2_text::vectorize::{TfidfVectorizer, Vectorizer};
use crate::tools::memory;

use super::base::{MemoryConfig, MemoryStore, MemoryItem, Memory};
use serde_json::Value;

pub struct WorkingMemory {
    config: MemoryConfig,
    store: MemoryStore,
    max_capacity: usize,
    max_age_minutes: i64,
    memories: Vec<MemoryItem>,
    session_start: i64,
}

impl WorkingMemory {
    pub fn new(config: MemoryConfig, store: MemoryStore) -> Self {
        let max_age_minutes = config.max_age_minutes.unwrap_or(60);
        let max_capacity = config.working_memory_capacoty.unwrap_or(50);
        Self {
            max_age_minutes,
            max_capacity,
            memories: vec![],
            session_start: Local::now().timestamp(),
            config,
            store,
        }
    }

    fn expire_old_memories(&mut self) {
        if self.memories.is_empty() {
            return;
        }

        let cutoff_time = Local::now().timestamp() - self.max_age_minutes * 60;
        let kept_memories: Vec<MemoryItem> = self.memories
            .clone()
            .into_iter()
            .filter(|memory| memory.timestamp >= cutoff_time)
            .collect();
        let removed_total = self.memories.len() - kept_memories.len();
        if removed_total == 0 {
            return;
        }
        self.memories = kept_memories;
    }

    fn remove(&mut self, memory_id: &String) {
        if let Some(index) = self.memories
            .iter()
            .position(|memory| memory_id == &memory.id) {
                // 先交换在删除
                self.memories.swap_remove(index);
            }
    }

    // 删除优先级最低的记忆
    fn remove_lowest_priority_memory(&mut self) {
        if self.memories.is_empty() {
            return;
        }
        // 找到优先级最低的记忆
        let mut lowest_priority = f64::INFINITY;
        let mut lowest_memory_id: Option<_> = None;
        for memory in &self.memories {
            let priority = self.calculate_priority(memory);
            if priority < lowest_priority {
                lowest_priority = priority;
                lowest_memory_id = Some(memory.id.clone());
            }
        }
        if let Some(memory_id) = lowest_memory_id {
            self.remove(&memory_id);
        }
    }

    fn try_tfidf_search(&self, query: &String) -> HashMap<String, f64> {

        if self.memories.is_empty() {
            return HashMap::new();
        }
    
        // 文本相识度计算
        let filter_memory = self.memories
            .iter()
            .map(|memory| memory)
            .collect::<Vec<_>>();
        
        // TF-IDF vectorization
        let mut documents = filter_memory
            .iter()
            .map(|memory| memory.content.as_str())
            .collect::<Vec<&str>>();
        documents.insert(0, query.as_str());

        // todo: 这里没有做 lowercase 可能影响最后的结果
        let mut tfidf = TfidfVectorizer::new(false, true, Some("l2".to_string()));
        let matrix    = match tfidf.fit_transform(&documents) {
            Ok(vectors) => vectors,
            Err(_) => return HashMap::new(),
        };

        // 计算cosine相识度
        let query_vec = matrix.row(0);  // 查询向量
        let mut similarities = Vec::with_capacity(filter_memory.len());
        // 计算所有的相识度然后zip压缩，成 memory_id : scorce

        for i in 1..matrix.nrows() {
            let memory_vec = matrix.row(i); // 记忆向量
            let similarity = query_vec.dot(&memory_vec);  // L2 归一化后的点积=于consine相似度
            similarities.push(similarity);
        }

        filter_memory
            .into_iter()
            .zip(similarities.into_iter())
            .map(|(memory, score)| (memory.id.to_string(), score))
            .collect::<HashMap<_, _>>()
    }

    fn calculate_keyword_score(&self, query: &String, content: &String) -> f64 {
        let query_lower = query.to_lowercase();
        let content_lower = content.to_lowercase();

        if content_lower.contains(&query_lower) {
            query_lower.len() as f64 / content_lower.len() as f64
        } else {
            let query_words: HashSet<&str> = query_lower.split_whitespace().collect();
            let content_words: HashSet<&str> = content_lower.split_whitespace().collect();

            let intersection = query_words.intersection(&content_words).count();
            if intersection > 0 {
                let union = query_words.union(&content_words).count();
                intersection as f64 / union as f64 * 0.8
            } else {
                0.0
            }
        }
    }

    // 计算记忆优先级
    fn calculate_priority(&self, memory: &MemoryItem) -> f64 {
        // 基础优先级 = 重要性
        let mut priority = memory.importance;
        // 时间衰减
        let time_decay = self.calculate_time_decay(memory.timestamp);
        priority *= time_decay;
        
        priority
    }

    fn calculate_time_decay(&self, timestamp: i64) -> f64 {
        let time_diff = Local::now().timestamp() - timestamp;
        let hours_passed = time_diff / 3600;

        let decay_factor = self.config.time_factor.powi(hours_passed as i32);

        decay_factor.max(0.1)
    }
}


impl Memory for WorkingMemory {
    fn add(&mut self, memory_item: MemoryItem) -> String {
        self.expire_old_memories();

        if self.memories.len() >= self.max_capacity {
            self.remove_lowest_priority_memory();
        }
        let memory_id = memory_item.id.clone();

        self.memories.push(memory_item);

        return memory_id;
    }

    fn retrieve(&mut self, query: &String, limit: Option<usize>, kwargs: Option<Value>) -> Vec<MemoryItem> {
        let limit = limit.unwrap_or(5);

        self.expire_old_memories();
        
        // TF-IDF 向量检索
        let vector_scores = self.try_tfidf_search(query);

        // 计算综合分数
        let mut scored_memories = vec![];
        for memory in self.memories.iter() {
            let vector_score = vector_scores.get(&memory.id).copied().unwrap_or(0.0);
            let keyword_score = self.calculate_keyword_score(query, &memory.content);
            
            // 混合评分
            let base_relevance = {
                let blance_score = vector_score * 0.7 + keyword_score * 0.3;
                if blance_score > 0f64 {
                    blance_score
                } else {
                    keyword_score
                }
            };

            let time_decay = self.calculate_time_decay(memory.timestamp);
            let importance_weight = 0.8 + (memory.importance * 0.4);

            let final_score = base_relevance * time_decay * importance_weight;

            if final_score > 0f64 {
                scored_memories.push((final_score, memory.clone()))
            }

        }

        scored_memories.sort_by(|a, b| b.0.total_cmp(&a.0));

        scored_memories
            .into_iter()
            .take(limit)
            .map(|(_, memory)| memory)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::base::{MemoryConfig, MemoryStore, MemoryItem};

    #[test]
    fn test_keyword_score_exact_match() {
        let wm = WorkingMemory::new(MemoryConfig::new(), MemoryStore::new(MemoryConfig::new()));
        let score = wm.calculate_keyword_score(&"蓝色".to_string(), &"我最喜欢的颜色是蓝色".to_string());
        assert!(score > 0.0);
    }

    #[test]
    fn test_keyword_score_word_overlap() {
        let wm = WorkingMemory::new(MemoryConfig::new(), MemoryStore::new(MemoryConfig::new()));
        let score = wm.calculate_keyword_score(&"favorite color".to_string(), &"my favorite color is blue".to_string());
        assert!(score > 0.0);
    }

    #[test]
    fn test_working_memory_retrieve() {
        let mut wm = WorkingMemory::new(MemoryConfig::new(), MemoryStore::new(MemoryConfig::new()));
        wm.add(MemoryItem {
            id: "1".to_string(),
            memory_type: "working".to_string(),
            content: "我最喜欢的颜色是蓝色".to_string(),
            timestamp: Local::now().timestamp(),
            importance: 0.8,
        });

        let results = wm.retrieve(&"喜欢的颜色".to_string(), Some(5), None);
        assert!(!results.is_empty(), "应该能检索到相关记忆");
        assert!(results.iter().any(|m| m.content.contains("蓝色")));
    }
}
