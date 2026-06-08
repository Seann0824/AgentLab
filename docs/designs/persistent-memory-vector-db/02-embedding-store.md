# 🧠 持久化记忆系统 — 基于向量数据库的技术设计 — Embedding 与本地向量存储

> 原文拆分自 `../persistent-memory-vector-db.md`。

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
