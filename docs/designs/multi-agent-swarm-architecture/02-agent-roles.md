# 🐝 多 Agent 蜂群架构设计（Multi-Agent Swarm Architecture） — Agent 角色定义

> 原文拆分自 `../multi-agent-swarm-architecture.md`。


### 4.1 🧠 Orchestrator Agent（调度者/主 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `orchestrator` |
| **数量** | 1（单例） |
| **生命周期** | 与主进程共存亡 |
| **运行模式** | 交互式（CLI）+ 后台调度 |

**职责：**
1. 用户交互入口（接受输入、显示输出）
2. 维护蜂群注册表（Swarm Registry）
3. 任务编排与派发（Task Dispatching）
4. 结果聚合与冲突解决
5. Agent 健康监控与重启
6. 上下文压缩与任务恢复

**特有工具：**
- `dispatch_task(agent_type, task, params)` — 派发任务给指定 Agent
- `spawn_agent_type(agent_type, config)` — 启动指定类型的新 Agent
- `query_swarm()` — 查询所有活跃 Agent 状态
- `broadcast(event, payload)` — 广播事件

**与当前系统的关系：**
- Orchestrator = 当前的 Agent（主循环），保持完全向后兼容
- 新增蜂群管理能力，但不影响现有的单 Agent 模式

---

### 4.2 📌 Memory Agent（记忆 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `memory` |
| **数量** | 1（单例） |
| **生命周期** | 常驻（随 Orchestrator 启动） |
| **运行模式** | 异步后台 |

**职责：**
1. **自动记忆提取** — 每 N 轮对话自动扫描上下文，提取重要信息存入向量数据库
2. **记忆总结** — 将分散的记忆合并、去重、生成摘要
3. **记忆检索** — 提供高效的内存向量检索服务
4. **记忆遗忘** — 根据重要性评分自动清理低价值记忆
5. **关系挖掘** — 发现记忆之间的关联（如「A 项目用到了 B 技术」）

**特有工具：**
- `auto_extract(context_messages)` — 从对话上下文自动提取可记忆的信息
- `consolidate_memories()` — 合并重复/相似记忆
- `forget_low_importance(threshold)` — 清理低重要性记忆

**与当前 MemoryManager 的关系：**
- 当前 MemoryManager 作为存储层（读/写向量 DB）
- Memory Agent 是上层逻辑：**何时**提取、**什么**值得记、**如何**总结
- Memory Agent 持有一个简化版的 ContextManager（可感知对话上下文）

**启动命令示例：**
```bash
agent-lab --agent-type memory --socket /tmp/agent-lab/memory.sock
```

---

### 4.3 🔧 General Agent（通用任务 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `general` |
| **数量** | 0~N（可伸缩） |
| **生命周期** | 按需创建，任务完成后进入池待命 |
| **运行模式** | 非交互式（只接受任务、执行、返回） |

**职责：**
1. 执行 Orchestrator 派发的通用任务
2. 文件读取、代码搜索、代码修改
3. 工具调用的独立执行环境
4. 结果返回给 Orchestrator

**特点：**
- 无交互式 CLI（仅通过 IPC 通信）
- 有独立的 ContextManager（但只保留当前任务上下文）
- 可复用（任务完成后不清除，进入 Agent Pool）
- 支持超时取消

**使用场景：**
```
用户: "帮我同时调查 A、B、C 三个方向的可行性"
→ Orchestrator 派发 3 个 General Agent 并行执行
→ 各自完成任务后返回结果
→ Orchestrator 汇总给用户
```

---

### 4.4 ✅ Verifier Agent（验证 Agent）

| 属性 | 值 |
|------|-----|
| **角色名** | `verifier` |
| **数量** | 0~3 |
| **生命周期** | 按需创建 |
| **运行模式** | 非交互式 |

**职责：**
1. **代码验证** — 修改代码后运行 `cargo check` / `cargo test`
2. **回归测试** — 运行预定义的测试套件
3. **端到端验证** — 运行场景测试
4. **编译优化** — 检测编译错误并分析根因
5. **质量门禁** — 决定代码修改是否通过质量检查

**特有工具：**
- `run_cargo_check(path)` — 运行编译检查
- `run_tests(test_filter)` — 运行测试
- `analyze_build_error(error_output)` — 分析编译错误

**与当前 spawn_agent 的关系：**
- 当前 spawn_agent = 编译 + 派生子进程执行任务（重、慢）
- Verifier Agent = 预编译、常驻、快速验证（轻、快）
- 当代码修改后，Orchestrator 通知 Verifier Agent 验证

---

### 4.5 📖 Reader Agent（阅读 Agent）— *可选*

| 属性 | 值 |
|------|-----|
| **角色名** | `reader` |
| **数量** | 0~N |
| **生命周期** | 按需创建 |
| **运行模式** | 非交互式 |

**职责：**
1. 专注阅读大型文件（Orchestrator 上下文放不下时）
2. 多文件关联分析
3. 代码结构理解与总结
4. 生成结构化分析报告

---

### 4.6 💻 Code Agent（编码 Agent）— *可选*

| 属性 | 值 |
|------|-----|
| **角色名** | `coder` |
| **数量** | 0~N |
| **生命周期** | 按需创建 |
| **运行模式** | 非交互式 |

**职责：**
1. 专注代码生成与修改
2. 多文件重构
3. 代码评审
4. 与 Verifier Agent 联动（修改→验证循环）

---

## 5. Agent 间通信模式
