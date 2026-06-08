# 🧠 持久化记忆系统 — 基于向量数据库的技术设计 — 记忆生命周期与 Agent 集成

> 原文拆分自 `../persistent-memory-vector-db.md`。


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

