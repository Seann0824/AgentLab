# Agent 能力路线图 — 自我进化与多 Agent 协作

> 原文拆分自 `../agent-capability-roadmap.md`。

## 5. 阶段二：自我进化能力（P1）

> **目标**: Agent 能够自主发现能力不足并扩展自身
> **预期效果**: Agent 可以「自举」——自己添加新工具、修复自己的 bug
> **估算**: 2-3 天

### 5.1 核心机制：内省（Introspection）

Agent 需要知道：
1. 我有哪些工具？ → 查看 `src/tools/` 目录
2. 工具是如何注册的？ → 查看 `initial_tool_manager()`
3. 如何添加新工具？ → 实现 `Tool` trait，注册到 `ToolManager`

#### 5.1.1 工具模板

提供一个**工具模板文件**，当 Agent 决定添加新工具时，可以复制模板并修改：

```rust
// src/tools/my_tool/mod.rs
// 模板：复制此文件并修改

use tokio::{sync::mpsc, fs};
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "我的工具描述" }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "my_tool",
                "description": "...",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" },
                    },
                    "required": ["input"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(async move {
            // 实现工具逻辑
            let _ = tx.send(ToolEvent::Done(serde_json::json!({
                "result": "done"
            }))).await;
        });
        Box::pin(ReceiverStream::new(rx))
    }
}
```

#### 5.1.2 注册步骤

Agent 添加新工具的步骤：
1. 创建 `src/tools/my_tool/mod.rs`
2. 在 `src/tools/mod.rs` 中注册：`pub mod my_tool;`
3. 在 `main.rs` 的 `initial_tool_manager()` 中注册：`tool_manager.register_tool(Box::new(MyTool));`
4. 运行 `cargo check` 验证

### 5.2 具体设计

#### 5.2.1 系统提示词中的自我进化指令

```
【自我进化】
你可以修改自身代码来增强能力：
1. 添加新工具 → 实现 Tool trait，注册到 ToolManager
2. 修改现有工具 → 增强已有工具的能力
3. 修复 bug → 修改对应 src/ 下的代码

规则：
- 修改代码后必须运行 cargo check 验证
- 验证通过后，新能力立即生效（下次调用时）
- 如果修改导致编译失败，分析错误并修复
- 重大改动建议分步进行（先加框架，再填实现）
```

#### 5.2.2 建议的初始工具集扩展

| 工具 | 优先级 | 说明 |
|------|--------|------|
| `read` (文件读取) | P1 | 更高效的文件读取，支持行号范围 |
| `search` (全文搜索) | P1 | 搜索目录中的文本内容 |
| `grep` (正则搜索) | P1 | 正则匹配搜索 |
| `plan` (计划管理) | P1 | 查看/更新 PLAN.md 状态 |
| `fetch` (网络请求) | P2 | HTTP GET 请求获取外部信息 |

---

## 6. 阶段三：多 Agent 协作（P2）

> **目标**: 支持多个 Agent 角色协同工作
> **预期效果**: 能处理需要不同专业知识的复杂任务
> **估算**: 3-5 天

### 6.1 架构设计

```
┌──────────────────────────────────────────┐
│           Orchestrator Agent              │
│  任务分解、分配、进度管理、结果汇总        │
└────┬────────┬────────┬────────┬───────────┘
     │        │        │        │
     ▼        ▼        ▼        ▼
┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐
│ Planner│ │ Coder │ │Tester │ │Reviewer│
│ 架构师 │ │ 编码  │ │ 测试  │ │ 审查   │
└──────┘ └──────┘ └──────┘ └──────┘
```

### 6.2 角色定义

| 角色 | 职责 | 工具集 |
|------|------|--------|
| **Orchestrator** | 任务分解、分配、协调 | 全部 |
| **Planner** | 需求分析、架构设计、计划输出 | read, search, plan |
| **Coder** | 编码实现、bug 修复 | read, edit, shell |
| **Tester** | 编写测试、运行测试、报告覆盖率 | shell, read |
| **Reviewer** | Code Review、质量检查 | read, search, shell |

### 6.3 实现方式

多 Agent 在初期可以通过「同一个模型实例，不同系统提示词」实现：

```
Agent 角色切换流程：
1. Orchestrator 分析任务
2. Orchestrator 写出子任务说明（写入文件）
3. 切换到 Coder Agent（加载 Coder 系统提示词）
4. Coder 执行完毕后，Orchestrator 验证结果
5. 切换到 Reviewer Agent...
```

更成熟的方案是使用多个 `ModelAdapter` 实例并行执行。

---

