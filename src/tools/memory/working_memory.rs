use std::{collections::HashMap, hash::Hash, vec};
use scirs2_text::vectorize::{TfidfVectorizer, Vectorizer};
use super::base::{MemoryConfig, MmeoryStore, MemoryItem, Memory};
use serde_json::Value;

pub struct WorkingMemory {
    max_capacity: usize,
    max_age_minutes: usize,
    memories: Vec<MemoryItem>,
}

impl WorkingMemory {
    pub fn new(config: MemoryConfig, _store: MmeoryStore) -> Self {
        let max_age_minutes = config.max_age_minutes.unwrap_or(60);
        let max_capacity = config.working_memory_capacoty.unwrap_or(50);
        Self {
            max_age_minutes,
            max_capacity,
            memories: vec![],
        }
    }

    fn expire_old_memories(&mut self) {

    }

    fn remove_lowest_priority_memory(&mut self) {

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
        0.0
    }

    fn calculate_time_decay(&self, timestamp: u64) -> f64 {
        0.0
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
