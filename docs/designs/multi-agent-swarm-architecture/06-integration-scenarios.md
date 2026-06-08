# 🐝 多 Agent 蜂群架构设计（Multi-Agent Swarm Architecture） — 集成点与示例场景

> 原文拆分自 `../multi-agent-swarm-architecture.md`。

## 附录 A：与现有系统的集成点

### A.1 对 agent.rs 的修改

```rust
// 当前 Agent 结构体
pub struct Agent {
    // ... 现有字段 ...
    // 🆕 新增字段
    swarm_registry: Option<SwarmRegistry>,     // 蜂群注册表（Orchestrator 模式）
    swarm_client: Option<UdsClient>,            // 蜂群客户端（子 Agent 模式）
}

// Agent::run() 修改
impl Agent {
    pub async fn run(&mut self) -> anyhow::Result<()> {
        if self.is_orchestrator() {
            // 1. 启动 UDS 服务器（监听 Agent 注册）
            // 2. 启动 Memory Agent（自动）
            // 3. 启动原有的交互循环（增强）
            // 4. 处理 Agent 注册/心跳/任务结果
        } else {
            // 1. 注册到 Orchestrator
            // 2. 监听任务并执行
            // 3. 返回结果
        }
        Ok(())
    }
}
```

### A.2 对 ToolManager 的修改

```rust
// 新增工具
tool_manager.register_tool(Box::new(DispatchTask {
    swarm_registry: registry.clone(),
}));

tool_manager.register_tool(Box::new(QuerySwarm {
    swarm_registry: registry.clone(),
}));
```

### A.3 对模型管理的集成

每个 Agent 类型可以有不同的模型配置：

```rust
/// Agent 类型与模型映射
pub struct AgentModelMap {
    /// orchestrator → "deepseek"
    /// memory → "deepseek" (轻量模型)
    /// general → "deepseek"
    /// verifier → "deepseek" (快速模型)
    mappings: HashMap<AgentType, String>,
}
```

---

## 附录 B：示例场景

### 场景 1：并行调研

```
用户: "帮我调研 Rust 中 3 种异步运行时（tokio、async-std、smol）的优劣"

Orchestrator:
  ├── 派发 General Agent #1 → 调研 tokio
  ├── 派发 General Agent #2 → 调研 async-std
  ├── 派发 General Agent #3 → 调研 smol
  │
  ├── (等待所有结果)
  │
  └── 汇总结果给用户
```

### 场景 2：自动迭代修复

```
用户: "实现一个 CSV 解析器"

Orchestrator:
  ├── 派发 Coder Agent → 实现 CSV 解析器代码
  │
  ├── 代码完成后 → 派发 Verifier Agent
  │     ├── 运行 cargo check → 发现编译错误
  │     └── 返回错误详情
  │
  ├── 分析错误 → 派发 Coder Agent → 修复
  │
  ├── 再次派发 Verifier Agent → 验证通过
  │
  └── 完成，通知用户
```

### 场景 3：长期记忆自动维护

```
Memory Agent (后台运行):
  ├── 每 5 轮对话后:
  │     ├── 扫描最近上下文
  │     ├── 提取重要信息（如技术选型、架构决策）
  │     ├── 与已有记忆对比，去重
  │     └── 存储到向量数据库
  │
  ├── 每 50 轮对话后:
  │     ├── 执行记忆合并
  │     ├── 清理低价值记忆
  │     └── 更新记忆重要性评分
  │
  └── 压缩发生时:
        └── 自动检索相关记忆注入上下文
```

---

> **文档版本**: v1.0
> **更新日期**: 2025-06-08
> **作者**: Agent Lab 架构团队
> **审批状态**: 📋 待 review
