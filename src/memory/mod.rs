// src/memory/mod.rs
//
// 持久化记忆系统模块入口。
//
// 基于向量数据库的长期记忆系统，支持：
// - 文本嵌入（调用 LLM embeddings API）
// - 本地文件向量存储 + 余弦相似度搜索
// - 记忆 CRUD 操作
// - 跨会话记忆注入
// - 记忆生命周期管理

pub mod embedding;
pub mod manager;
pub mod store;
pub mod types;

pub use embedding::EmbeddingClient;
pub use manager::MemoryManager;
pub use store::VectorStore;
pub use types::*;
