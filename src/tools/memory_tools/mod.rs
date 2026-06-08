// src/tools/memory_tools/mod.rs
//
// 记忆工具集 — Agent 可通过这些工具操作长期记忆系统。
//
// 包含：
// - MemorySaveTool: 保存重要信息到长期记忆
// - MemorySearchTool: 搜索相关记忆
// - MemoryForgetTool: 删除不需要的记忆
// - MemoryStatsTool: 查看记忆系统状态

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::memory::{MemoryManager, MemorySource};
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// 记忆保存工具
pub struct MemorySaveTool {
    pub memory_manager: Arc<Mutex<MemoryManager>>,
}

impl Tool for MemorySaveTool {
    fn name(&self) -> &str {
        "memory_save"
    }

    fn description(&self) -> &str {
        "Save important information to long-term memory. The information will be vectorized and searchable across sessions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "memory_save",
                "description": self.description(),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "The information content to remember"
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional tags for categorization (e.g. ['user-preference', 'python'])"
                        },
                        "importance": {
                            "type": "number",
                            "description": "Importance score 0.0-1.0 (default 0.5). Higher = more likely to be recalled.",
                            "default": 0.5
                        }
                    },
                    "required": ["content"]
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let memory_manager = self.memory_manager.clone();
        let content = args["content"].as_str().unwrap_or("").to_string();
        let tags: Vec<String> = args["tags"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let importance = args["importance"].as_f64().unwrap_or(0.5) as f32;

        Box::pin(async_stream::stream! {
            if content.is_empty() {
                yield ToolEvent::Err("content is required".to_string());
                return;
            }

            let mut mgr = memory_manager.lock().await;
            match mgr.save(&content, &tags, MemorySource::Manual, importance).await {
                Ok(id) => {
                    yield ToolEvent::Done(serde_json::json!({
                        "ok": true,
                        "id": id,
                        "message": format!("Memory saved successfully (id: {})", id)
                    }));
                }
                Err(e) => {
                    yield ToolEvent::Err(format!("Failed to save memory: {}", e));
                }
            }
        })
    }
}

/// 记忆搜索工具
pub struct MemorySearchTool {
    pub memory_manager: Arc<Mutex<MemoryManager>>,
}

impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search long-term memory for relevant information using semantic similarity. Returns memories sorted by relevance."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "memory_search",
                "description": self.description(),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query describing what you're looking for"
                        },
                        "top_k": {
                            "type": "integer",
                            "description": "Maximum number of results to return (default 5, max 20)",
                            "default": 5
                        }
                    },
                    "required": ["query"]
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let memory_manager = self.memory_manager.clone();
        let query = args["query"].as_str().unwrap_or("").to_string();
        let top_k = args["top_k"].as_u64().unwrap_or(5).min(20) as usize;

        Box::pin(async_stream::stream! {
            if query.is_empty() {
                yield ToolEvent::Err("query is required".to_string());
                return;
            }

            let mgr = memory_manager.lock().await;
            match mgr.search_similar(&query, top_k).await {
                Ok(results) => {
                    let memories: Vec<serde_json::Value> = results.iter().map(|r| {
                        serde_json::json!({
                            "id": r.record.id,
                            "content": r.record.content,
                            "score": format!("{:.2}", r.score),
                            "tags": r.record.tags,
                            "importance": r.record.importance,
                            "source": r.record.source,
                            "created_at": r.record.created_at,
                            "accessed_at": r.record.accessed_at,
                            "access_count": r.record.access_count,
                        })
                    }).collect();

                    yield ToolEvent::Done(serde_json::json!({
                        "ok": true,
                        "count": memories.len(),
                        "memories": memories,
                    }));
                }
                Err(e) => {
                    yield ToolEvent::Err(format!("Failed to search memory: {}", e));
                }
            }
        })
    }
}

/// 记忆删除工具
pub struct MemoryForgetTool {
    pub memory_manager: Arc<Mutex<MemoryManager>>,
}

impl Tool for MemoryForgetTool {
    fn name(&self) -> &str {
        "memory_forget"
    }

    fn description(&self) -> &str {
        "Delete a specific memory entry by its ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "memory_forget",
                "description": self.description(),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "The ID of the memory to delete (e.g. 'mem_abc123_0')"
                        }
                    },
                    "required": ["id"]
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let memory_manager = self.memory_manager.clone();
        let id = args["id"].as_str().unwrap_or("").to_string();

        Box::pin(async_stream::stream! {
            if id.is_empty() {
                yield ToolEvent::Err("id is required".to_string());
                return;
            }

            let mut mgr = memory_manager.lock().await;
            if mgr.forget(&id) {
                let _ = mgr.flush();
                yield ToolEvent::Done(serde_json::json!({
                    "ok": true,
                    "message": format!("Memory '{}' forgotten", id)
                }));
            } else {
                yield ToolEvent::Err(format!("Memory '{}' not found", id));
            }
        })
    }
}

/// 记忆统计工具
pub struct MemoryStatsTool {
    pub memory_manager: Arc<Mutex<MemoryManager>>,
}

impl Tool for MemoryStatsTool {
    fn name(&self) -> &str {
        "memory_stats"
    }

    fn description(&self) -> &str {
        "View memory system statistics and configuration."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "memory_stats",
                "description": self.description(),
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        })
    }

    fn execute(&self, _args: serde_json::Value) -> ToolStream {
        let memory_manager = self.memory_manager.clone();

        Box::pin(async_stream::stream! {
            let mgr = memory_manager.lock().await;
            let stats = mgr.stats();
            let config = mgr.config_info();

            yield ToolEvent::Done(serde_json::json!({
                "ok": true,
                "config": config,
                "stats": {
                    "total_entries": stats.total_entries,
                    "last_compaction": stats.last_compaction,
                    "vector_dim": stats.vector_dim,
                },
                "memories": mgr.list(10).iter().map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "content": e.content,
                        "importance": e.importance,
                        "tags": e.tags,
                        "source": e.source,
                    })
                }).collect::<Vec<_>>(),
            }));
        })
    }
}

// Re-export async-stream for use in the stream macro
use async_stream;
