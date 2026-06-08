# 🧠 持久化记忆系统 — 基于向量数据库的技术设计 — 配置、依赖、实施与风险

> 原文拆分自 `../persistent-memory-vector-db.md`。

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
