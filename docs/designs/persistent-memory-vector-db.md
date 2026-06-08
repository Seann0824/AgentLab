# 🧠 持久化记忆系统 — 基于向量数据库的技术设计

> **版本**: v1.0  
> **创建日期**: 2025-06-14  
> **状态**: 设计阶段  
> **对应路线图**: Phase 3 — 持久记忆

---

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

## 3. Embedding API 集成

### 3.1 实现方式

复用已有的 `OpenAiCompatibleAdapter` 的配置（`base_url`、`api_key`），调用 **`/embeddings`** 端点。

```
POST {base_url}/embeddings
Authorization: Bearer {api_key}
Content-Type: application/json

{
  "model": "{model_name}",
  "input": "要向量化的文本内容"
}
```

### 3.2 EmbeddingClient 设计

```rust
pub struct EmbeddingClient {
    base_url: String,
    api_key: String,
    model: String,        // embedding model name (e.g. "text-embedding-3-small")
    client: reqwest::Client,
    vector_dim: usize,    // 向量维度 (e.g. 1536 for text-embedding-3-small)
}

impl EmbeddingClient {
    /// 从现有的 ModelAdapter 配置创建
    pub fn from_model_config(config: &ModelConfig) -> Self;
    
    /// 生成单个文本的嵌入向量
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
    
    /// 批量生成嵌入向量
    pub async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;
    
    /// 获取向量维度
    pub fn vector_dim(&self) -> usize;
}
```

### 3.3 配置发现

EmbeddingClient 的配置通过环境变量自动发现（同 ModelManager 的发现机制）：
- `LLM_API_KEY` → api_key
- `LLM_BASE_URL` → base_url
- `LLM_EMBEDDING_MODEL` → 嵌入模型名（默认 `text-embedding-3-small`）

如未配置 `LLM_EMBEDDING_MODEL`，则自动推断：
- 如果 chat model 是 `gpt-4*` → 使用 `text-embedding-3-small`
- 如果 chat model 是 `claude-*` → 使用 `voyage-2`（需配置）

---

## 4. 本地向量存储（VectorStore）

### 4.1 存储设计

使用基于文件的向量存储，核心策略：

1. **文件级索引**: index.json 维护所有记忆条目的元数据（不含向量），支持快速过滤
2. **向量分布存储**: vectors/ 目录下按 id 前缀分片存储向量数据，避免单个文件过大
3. **延迟加载**: 搜索时只加载必要的向量分片，而非全量加载

### 4.2 余弦相似度搜索

```rust
/// 计算两个向量之间的余弦相似度 (-1 ~ 1)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b + 1e-10)  // 防止除零
}
```

### 4.3 VectorStore API

```rust
pub struct VectorStore {
    store_dir: PathBuf,
    index: StoreIndex,
    vector_dim: usize,
}

impl VectorStore {
    /// 打开或创建向量存储
    pub fn open(store_dir: PathBuf, vector_dim: usize) -> anyhow::Result<Self>;
    
    /// 插入一条向量记录
    pub async fn insert(&mut self, record: VectorRecord) -> anyhow::Result<()>;
    
    /// 搜索最相似的 K 条记录
    pub async fn search(&self, query_vector: &[f32], top_k: usize) -> anyhow::Result<Vec<SearchResult>>;
    
    /// 按标签过滤 + 语义搜索
    pub async fn search_with_tags(&self, query_vector: &[f32], tags: &[String], top_k: usize) -> anyhow::Result<Vec<SearchResult>>;
    
    /// 按 ID 删除
    pub async fn delete(&mut self, id: &str) -> anyhow::Result<bool>;
    
    /// 更新重要性评分
    pub async fn update_importance(&mut self, id: &str, importance: f32) -> anyhow::Result<()>;
    
    /// 获取存储统计信息
    pub fn stats(&self) -> StoreStats;
    
    /// 压缩存储（合并碎片、剔除低重要性条目）
    pub async fn compact(&mut self, min_importance: f32) -> anyhow::Result<usize>;
}

pub struct SearchResult {
    pub record: VectorRecord,
    pub score: f32,  // 余弦相似度分数
}
```

### 4.4 搜索策略优化

1. **Top-K + 阈值过滤**: 返回 Top-K 结果后，过滤掉低于相似度阈值（默认 0.6）的结果
2. **混合搜索**: 标签精确匹配 + 向量语义搜索的组合
3. **重要性加权**: 最终排序中，重要性评分作为权重因子 `final_score = similarity * 0.7 + importance * 0.3`

---

## 5. 记忆生命周期管理

### 5.1 记忆流程

```
                    ┌──────────────┐
                    │  对话/工具输出 │
                    └──────┬───────┘
                           ▼
                    ┌──────────────┐
                    │ 记忆提取     │ ← 规则检测 + LLM 辅助判断
                    │ "这段值得记住"│
                    └──────┬───────┘
                           ▼
                    ┌──────────────┐
                    │ Embedding    │ ← 调用 LLM API 生成向量
                    └──────┬───────┘
                           ▼
                    ┌──────────────┐
                    │ VectorStore  │ ← 存储到文件
                    │ .memory/     │
                    └──────┬───────┘
                           ▼
              ┌────────────────────────┐
              │  下次对话启动时        │
              │  MemoryManager 查询    │
              │  相关记忆 → 注入上下文 │
              └────────────────────────┘
```

### 5.2 自动提取规则

在 Agent 主循环的工具执行结果或对话中，系统自动检测以下「值得记住」的场景：

| 条件 | 示例 | 重要性 |
|------|------|--------|
| 用户明确要求记住 | "请记住我喜欢的编程语言是 Rust" | 0.9 |
| 用户偏好/配置信息 | "我习惯用 neovim 编辑器" | 0.8 |
| 项目关键决策 | "我们决定使用 PostgreSQL 作为数据库" | 0.7 |
| 重复出现的信息 | 多次提及相同的技术栈 | 0.6 |
| 工具执行的关键发现 | 调试结果中的重要配置信息 | 0.5 |

### 5.3 记忆注入机制

Agent 每次启动/上下文刷新时，MemoryManager 自动执行：

```
1. 获取当前对话的上文（最近的 N 条消息）
2. 使用 EmbeddingClient 将上下文向量化
3. 在 VectorStore 中搜索 Top-5 相关记忆
4. 将相关记忆注入到系统提示词中
```

注入格式：
```
[相关记忆]
以下是你之前记住的重要信息（按相关度排序）：

1. [记忆内容] (来源: 用户输入, 重要性: 0.85)
2. [记忆内容] (来源: 工具输出, 重要性: 0.72)
...
```

### 5.4 遗忘机制

| 策略 | 触发条件 | 行为 |
|------|---------|------|
| **重要性淘汰** | 存储量 > 阈值 (默认 1000 条) | 删除重要性 < 0.3 且访问次数 < 2 的条目 |
| **陈旧淘汰** | 超过 90 天未访问 | 自动归档到 `.memory/archive/` |
| **手动遗忘** | 用户 `/memory forget <id>` | 立即删除 |
| **压缩** | 每 100 次写入后 | 合并碎片、更新索引 |

---

## 6. Agent 集成方案

### 6.1 MemoryManager 接口

```rust
pub struct MemoryManager {
    embedding: EmbeddingClient,
    store: VectorStore,
    enabled: bool,
}

impl MemoryManager {
    /// 创建记忆管理器
    pub async fn new(store_dir: PathBuf) -> anyhow::Result<Self>;
    
    /// 保存一条记忆
    pub async fn save(&mut self, content: &str, tags: &[String], source: MemorySource, importance: f32) -> anyhow::Result<String>;
    
    /// 搜索相关记忆
    pub async fn search(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<SearchResult>>;
    
    /// 搜索相关记忆（使用文本，自动向量化）
    pub async fn search_similar(&self, text: &str, top_k: usize) -> anyhow::Result<Vec<SearchResult>>;
    
    /// 获取与当前上下文相关的记忆（用于注入）
    pub async fn get_relevant_memories(&self, context: &[&str], top_k: usize) -> anyhow::Result<Vec<SearchResult>>;
    
    /// 删除记忆
    pub async fn forget(&mut self, id: &str) -> anyhow::Result<bool>;
    
    /// 列出记忆（按重要性排序）
    pub async fn list(&self, limit: usize) -> anyhow::Result<Vec<VectorRecord>>;
    
    /// 获取记忆注入文本（用于系统提示词）
    pub async fn get_injection_text(&self, recent_messages: &[&str]) -> anyhow::Result<String>;
}
```

### 6.2 记忆工具

Agent 可通过以下工具操作记忆系统：

#### memory_save
```json
{
  "name": "memory_save",
  "description": "保存一条重要信息到长期记忆",
  "parameters": {
    "content": "要记住的信息内容",
    "tags": ["标签1", "标签2"],
    "importance": 0.7
  }
}
```

#### memory_search
```json
{
  "name": "memory_search",
  "description": "搜索相关记忆",
  "parameters": {
    "query": "搜索查询",
    "top_k": 5,
    "tags": ["可选", "标签过滤"]
  }
}
```

#### memory_forget
```json
{
  "name": "memory_forget",
  "description": "删除一条记忆",
  "parameters": {
    "id": "记忆ID"
  }
}
```

### 6.3 Agent 集成改动点

| 文件 | 改动 |
|------|------|
| `src/agent.rs` | 添加 `memory_manager: Option<MemoryManager>` 字段 |
| `src/agent.rs` | 启动时调用 `get_injection_text()` 注入相关记忆 |
| `src/agent.rs` | 主循环中检测「值得记住」的信息，自动调用 save() |
| `src/tools/mod.rs` | 注册 memory_save, memory_search, memory_forget 工具 |
| `src/lib.rs` | 添加 `pub mod memory;` |
| `Cargo.toml` | 可能无需改动（复用现有依赖） |

### 6.4 系统提示词注入

在系统提示词末尾，如果存在活跃的记忆系统，注入以下内容：

```
[长期记忆系统]
你有一个长期记忆系统，可以跨会话记住重要信息。
使用以下工具操作记忆：
- memory_save: 保存重要信息
- memory_search: 搜索相关记忆
- memory_forget: 删除不需要的记忆

{自动注入的相关记忆列表}
```

---

## 7. 配置项

所有配置通过环境变量控制：

| 环境变量 | 默认值 | 说明 |
|---------|--------|------|
| `LLM_BASE_URL` | (从 ModelManager 继承) | LLM API 基础地址 |
| `LLM_API_KEY` | (从 ModelManager 继承) | API Key |
| `LLM_EMBEDDING_MODEL` | `text-embedding-3-small` | 嵌入模型名 |
| `MEMORY_DIR` | `.memory` | 记忆存储目录 |
| `MEMORY_MAX_ENTRIES` | `1000` | 最大记忆条目数 |
| `MEMORY_SIMILARITY_THRESHOLD` | `0.6` | 相似度阈值 |
| `MEMORY_AUTO_EXTRACT` | `true` | 是否自动提取记忆 |

---

## 8. 依赖分析

### 现有依赖（无需新增）
| 依赖 | 用途 |
|------|------|
| `reqwest` | HTTP 调用 embeddings API ✅ 已有 |
| `serde` / `serde_json` | 序列化/反序列化 ✅ 已有 |
| `anyhow` | 错误处理 ✅ 已有 |

### 无需外部向量数据库
本项目采用 **纯文件 + 余弦相似度** 的轻量级方案，不引入外部向量数据库依赖：
- 零额外部署成本
- 对于 < 10000 条记忆，线性扫描足够快
- 后续可替换为 pgvector / qdrant 等外部存储

---

## 9. 实施路线

### Phase 1: 基础模块 (当前步骤 2-5)
```
Day 1: types.rs + mod.rs 框架
Day 1: EmbeddingClient 实现
Day 2: VectorStore 实现  
Day 2: MemoryManager 实现
Day 3: 编译验证
```

### Phase 2: Agent 集成 (当前步骤 7-10)
```
Day 3: 记忆工具实现
Day 4: Agent 注入 + 自动提取
Day 5: 完整集成验证
```

### Phase 3: 验证 (当前步骤 11-12)
```
Day 5: spawn_agent 端到端验证
Day 5: 更新 ROADMAP.md
```

---

## 10. 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| Embedding API 不可用 | 无法生成向量 | 降级为纯标签搜索 |
| 大规模记忆搜索慢 | 响应延迟 | 分片存储 + 索引优化 |
| 记忆存储膨胀 | 磁盘占用 | 遗忘机制 + 压缩 |
| 无关记忆注入 | 上下文污染 | 相似度阈值 + 重要性加权 |
