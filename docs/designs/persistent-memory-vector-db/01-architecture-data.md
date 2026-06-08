# 🧠 持久化记忆系统 — 基于向量数据库的技术设计 — 系统架构与数据结构

> 原文拆分自 `../persistent-memory-vector-db.md`。

## 1. 系统架构总览

```
┌──────────────────────────────────────────────────────────┐
│                     Agent 主循环                          │
│  ┌─────────────┐   ┌──────────────┐   ┌──────────────┐  │
│  │ 记忆注入     │ ← │ MemoryManager│ ← │ 记忆工具     │  │
│  │ (自动/手动)  │   │ (核心调度)   │   │ (save/search)│  │
│  └──────┬──────┘   └──────┬───────┘   └──────┬───────┘  │
└─────────┼─────────────────┼──────────────────┼──────────┘
          │                 │                  │
          ▼                 ▼                  ▼
┌──────────────────────────────────────────────────────────┐
│                     Memory 层                              │
│  ┌──────────────────┐   ┌──────────────────────────────┐  │
│  │   VectorStore    │   │   EmbeddingClient            │  │
│  │   (本地文件存储)  │   │   (调用 LLM embeddings API) │  │
│  └──────┬───────────┘   └──────────────┬───────────────┘  │
│         │                              │                  │
└─────────┼──────────────────────────────┼──────────────────┘
          │                              │
          ▼                              ▼
┌────────────────────┐   ┌────────────────────────────┐
│   .memory/ 目录    │   │   外部 LLM API             │
│   (JSON 文件)      │   │   POST /embeddings         │
└────────────────────┘   └────────────────────────────┘
```

### 核心组件

| 组件 | 职责 | 关键文件 |
|------|------|---------|
| **MemoryManager** | 记忆系统对外统一接口，协调 Embedding + VectorStore | `src/memory/manager.rs` |
| **EmbeddingClient** | 调用 LLM API 的 embeddings 端点生成文本向量 | `src/memory/embedding.rs` |
| **VectorStore** | 本地文件向量存储 + 余弦相似度搜索 | `src/memory/store.rs` |
| **MemoryTools** | Agent 可调用的记忆操作工具 | `src/tools/memory_tools/` |
| **MemoryEntry** | 记忆条目数据结构 | `src/memory/types.rs` |

### 数据流

```
对话 → 提取重要信息 → EmbeddingClient(向量化) → VectorStore(存储)
                                                      │
Agent启动 → MemoryManager(查询相关记忆) ←──────────────┘
                │
                ▼
        注入到上下文(系统提示词)
```

---

## 2. 数据结构设计

### 2.1 MemoryEntry — 记忆条目

```rust
/// 记忆条目：存储在向量数据库中的核心数据结构
pub struct MemoryEntry {
    /// 唯一标识 (UUID v4)
    pub id: String,
    
    /// 记忆内容（原始文本）
    pub content: String,
    
    /// 嵌入向量 (由 EmbeddingClient 生成)
    pub vector: Vec<f32>,
    
    /// 元数据标签
    pub tags: Vec<String>,
    
    /// 重要性评分 (0.0 ~ 1.0)
    pub importance: f32,
    
    /// 记忆来源（对话摘要、工具结果、用户输入等）
    pub source: MemorySource,
    
    /// 创建时间
    pub created_at: String,
    
    /// 最后访问时间（用于 LRU 淘汰）
    pub accessed_at: String,
    
    /// 访问次数（用于评估记忆热度）
    pub access_count: u32,
}

/// 记忆来源分类
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
}

/// 向量存储记录（序列化格式）
pub struct VectorRecord {
    pub id: String,
    pub content: String,
    pub vector: Vec<f32>,       // 嵌入向量
    pub tags: Vec<String>,
    pub importance: f32,
    pub source: String,
    pub created_at: String,
    pub accessed_at: String,
    pub access_count: u32,
}
```

### 2.2 文件存储结构

```
.memory/
├── index.json              # 记忆索引（id → 元数据，不含向量）
├── vectors/
│   ├── v_{id_prefix}.json  # 向量数据分片（按 id 前缀分片）
│   └── ...
└── config.json             # 存储配置（维度、阈值等）
```

**index.json 格式**:
```json
{
  "entries": [
    {
      "id": "mem_abc123",
      "content": "用户偏好使用 Python 进行数据分析",
      "tags": ["user-preference", "python", "data-science"],
      "importance": 0.85,
      "source": "UserInput",
      "created_at": "2025-06-14T10:30:00Z",
      "accessed_at": "2025-06-14T11:00:00Z",
      "access_count": 3
    }
  ],
  "stats": {
    "total_entries": 1,
    "last_compaction": "2025-06-14T12:00:00Z",
    "vector_dim": 1536
  }
}
```

---

