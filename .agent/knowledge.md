# 项目知识库

> Agent 对当前项目的技术栈、架构约定、编码规范的认知。
> 由 Agent 自动发现和维护。

---

## 技术栈

- 语言: Rust (edition 2024)
- 异步运行时: Tokio
- HTTP 客户端: reqwest
- 序列化: serde / serde_json
- 流处理: futures-util / tokio-stream

## 项目架构

```
src/
├── main.rs                    # 主循环入口
├── agent.rs                   # Agent 逻辑（预留）
├── context/                   # 上下文窗口管理
│   ├── mod.rs                 # ContextManager 核心
│   ├── config.rs              # 策略配置
│   ├── strategy.rs            # 压缩策略实现
│   ├── summarizer.rs          # 异步摘要生成
│   ├── tokenizer.rs           # Token 估算
│   └── types.rs               # 类型定义
├── model/                     # 模型适配层
│   ├── mod.rs                 # ModelAdapter trait
│   ├── types.rs               # ChatMessage, ModelEvent
│   └── openai_compatible.rs   # OpenAI 兼容 API
└── tools/                     # 工具系统
    ├── mod.rs                 # ToolManager
    ├── types.rs               # Tool trait
    ├── base_shell/            # Shell 工具
    └── edit_tool/             # 文件编辑工具
```

## 编码规范

- 使用 `anyhow::Result` 处理错误
- 工具使用 `ToolStream` (async Stream) 模式
- 重要变更需通过 cargo check 验证
