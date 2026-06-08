# 🎯 目标驱动能力（Goal-Driven Capability）

> **设计版本**: v1.0  
> **状态**: 设计中  
> **对应路线图**: 阶段一「结构化执行框架」的进化方向  
> **核心思想**: 让 Agent 从「反应式对话」进化为「目标驱动的自主执行体」

---

## 目录

1. [动机与目标](#1-动机与目标)
2. [核心概念](#2-核心概念)
3. [整体架构](#3-整体架构)
4. [Goal 数据结构](#4-goal-数据结构)
5. [Goal 生命周期](#5-goal-生命周期)
6. [自评估机制](#6-自评估机制)
7. [持久化执行循环](#7-持久化执行循环)
8. [与现有系统的集成](#8-与现有系统的集成)
9. [实现计划](#9-实现计划)
10. [使用示例](#10-使用示例)

---

## 1. 动机与目标

### 1.1 当前问题

当前 Agent 的行为模式是 **反应式（Reactive）**：

```
用户: "帮我实现 X 功能"
Agent: (执行步骤1) → (等待用户反馈) → (执行步骤2) → ...
```

这种模式的问题：
- **缺乏持续性**：每轮执行后等待用户输入，复杂任务需要反复唤醒
- **没有全局目标意识**：上下文压缩后可能丢失整体目标
- **无法自评估**：Agent 不判断"是否完成"，而是等用户说"继续"或"停"
- **没有完成标准**：没有显式的成功/失败判定

### 1.2 设计目标

- **目标驱动**：用户设定目标后，Agent 自主推进直到完成
- **自我评估**：Agent 自行判断目标是否达成，不需要用户干预
- **持久追踪**：目标状态持久化到文件，重启后继续
- **进度透明**：用户随时了解目标完成进度
- **弹性执行**：遇到错误自动重试/调整方案，不轻易放弃

---

## 2. 核心概念

```
┌──────────────────────────────────────────────────────────┐
│                     Goal-Driven Loop                       │
│                                                           │
│  用户设定 Goal                                              │
│       ↓                                                   │
│  ┌─────────────┐    ┌─────────────┐    ┌──────────────┐   │
│  │   Plan       │ →  │   Execute   │ →  │  Self-check  │   │
│  │  (规划)      │    │  (执行)     │    │  (自评估)    │   │
│  └─────────────┘    └─────────────┘    └──────┬───────┘   │
│       ↑                                       │            │
│       └───────────────────────────────────────┘            │
│           不满足完成条件 → 继续循环                           │
│                                                           │
│  满足完成条件 → 标记 Goal 完成 → 输出总结                   │
└──────────────────────────────────────────────────────────┘
```

### 2.1 关键术语

| 术语 | 英文 | 定义 |
|------|------|------|
| **目标** | Goal | 用户设定的一个可完成的任务目标 |
| **完成标准** | Completion Criteria | 判断 Goal 是否完成的显式条件 |
| **自评估** | Self-check | Agent 对当前工作成果的自我审查 |
| **目标状态** | Goal Status | Goal 的当前生命周期阶段 |
| **目标注册表** | Goal Registry | 所有 Goal 的持久化存储 |

---

## 3. 整体架构

### 3.1 模块依赖关系

```
┌─────────────────────────────────────────────────────┐
│                   Agent (agent.rs)                    │
│                                                       │
│  ┌─────────────────────────────────────────────────┐ │
│  │           Goal Manager (goal/)                   │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │ │
│  │  │  Goal     │  │  Goal    │  │  Self-check   │  │ │
│  │  │  Registry │  │  Engine  │  │  Evaluator   │  │ │
│  │  └──────────┘  └──────────┘  └──────────────┘  │ │
│  └─────────────────────────────────────────────────┘ │
│                       ↕                               │
│  ┌─────────────────────────────────────────────────┐ │
│  │           TaskManager (task/)                    │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │ │
│  │  │ PLAN.md  │  │AGENDA.md │  │  MEMORY.md   │  │ │
│  │  └──────────┘  └──────────┘  └──────────────┘  │ │
│  └───────────────────────────────────────────���─────┘ │
│                       ↕                               │
│  ┌─────────────────────────────────────────────────┐ │
│  │           ToolManager (tools/)                   │ │
│  │     shell | edit | read | search | spawn_agent  │ │
│  └─────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

### 3.2 设计原则

1. **增量实现**：在现有 TaskManager 基础上扩展，不重构现有代码
2. **文件持久化**：Goal 状态写入 `docs/goals/` 目录，支持跨会话恢复
3. **LLM 原生驱动**：Goal 的创建、评估、完成判断由 LLM 自主完成，代码只提供框架
4. **与 TaskManager 互补**：TaskManager 跟踪短期"步骤"，GoalManager 跟踪长期"目标"
5. **松耦合**：GoalManager 作为独立模块，通过 Agent 主循环集成

---

## 4. Goal 数据结构

### 4.1 核心数据模型

```rust
/// 目标状态枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalStatus {
    /// 已提议（用户刚设定，尚未激活）
    Proposed,
    /// 进行中（正在执行）
    Active,
    /// 已完成（自评估通过）
    Completed,
    /// 失败（自评估确认不可达成）
    Failed,
    /// 已取消（用户主动取消）
    Cancelled,
}

/// ⭐ 目标定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    /// 唯一标识（UUID）
    pub id: String,
    /// 目标名称（简短描述）
    pub name: String,
    /// 目标详细描述
    pub description: String,
    /// 完成标准（显式条件列表）
    pub criteria: Vec<String>,
    /// 当前状态
    pub status: GoalStatus,
    /// 执行进度 (0-100)
    pub progress: u8,
    /// 关联的 PLAN.md 步骤列表
    pub steps: Vec<String>,
    /// 已完成步骤
    pub completed_steps: Vec<String>,
    /// 关键决策记录
    pub decisions: Vec<String>,
    /// 创建时间
    pub created_at: String,
    /// 最后更新时间
    pub updated_at: String,
    /// 完成时间（当 status == Completed 时）
    pub completed_at: Option<String>,
}
```

### 4.2 持久化文件结构

```
docs/goals/
├── index.json              # Goal 注册表（所有 Goal 的摘要列表）
├── goal_<id>.json          # 每个 Goal 的完整数据
└── goal_<id>.log           # Goal 的执行日志（可选，详细记录）
```

**index.json 格式**：
```json
{
  "goals": [
    {
      "id": "abc123",
      "name": "实现用户登录功能",
      "status": "Active",
      "progress": 60,
      "updated_at": "2025-06-08 12:00"
    }
  ],
  "last_updated": "2025-06-08 12:00"
}
```

### 4.3 Goal 与 PLAN.md 的关联

Goal 不替代 PLAN.md，而是互补：

| 维度 | Goal | PLAN.md |
|------|------|---------|
| **粒度** | 粗粒度（整体目标） | 细粒度（具体步骤） |
| **生命周期** | 长（小时/天） | 短（分钟/小时） |
| **评估方式** | 自评估完成标准 | 勾选 checkbox |
| **数量** | 通常 1 个活跃 | 多个 |
| **持久化** | `docs/goals/*.json` | `docs/PLAN.md` |

关系：
- **1 个 Goal** 对应 **1 个 PLAN.md**（当前执行的计划）
- Goal 的 `steps` 字段与 PLAN.md 的 checkbox 列表同步
- Goal 完成 = PLAN.md 所有步骤完成 + 自评估通过

---

## 5. Goal 生命周期

### 5.1 状态转换图

```
                  ┌──────────┐
                  │ Proposed │
                  └─────┬────┘
                        │ 用户确认 / Agent 开始执行
                        ↓
                  ┌──────────┐
          ┌──────→│  Active  │←──────┐
          │       └─────┬────┘       │
          │             │            │
          │     ┌───────┴───────┐    │
          │     │               │    │
          │     ↓               ↓    │
          │  ┌──────────┐  ┌──────┐ │
          │  │Completed │  │Failed│ │
          │  └──────────┘  └──────┘ │
          │                         │
          └──────重新规划──────────┘
                 (从 Failed 恢复)

                  ┌──────────┐
                  │Cancelled │ (用户主动取消)
                  └──────────┘
```

### 5.2 状态转换条件

| 从 → 到 | 触发条件 | 执行者 |
|----------|---------|--------|
| Proposed → Active | Agent 开始执行 Goal | Agent |
| Active → Completed | 所有步骤完成 + 自评估通过 | Agent |
| Active → Failed | 自评估判定不可达成 / 多次重试失败 | Agent |
| Active → Cancelled | 用户明确要求取消 | 用户 |
| Failed → Active | 重新规划后继续执行 | Agent |
| 任意 → Proposed | 用户修改目标描述 | 用户/Agent |

---

## 6. 自评估机制

### 6.1 自评估时机

Agent 在以下时机进行自评估：

1. **步骤完成后**：每完成一个步骤，检查该步骤是否满足预期
2. **达到检查点**：每 N 步或累计一定工作量后
3. **全部步骤完成后**：最终验收评估
4. **遇到严重错误后**：评估是否应该调整方案或标记失败

### 6.2 自评估流程

```
┌─────────────────────────────────────┐
│         自评估 (Self-Check)          │
│                                     │
│  1. 回顾完成标准 (criteria)          │
│  2. 逐条检查是否满足                  │
│     - 代码编译检查 (cargo check)     │
│     - 功能逻辑验证                    │
│     - 文件完整性检查                  │
│  3. 输出评估结果                      │
│     - ✅ 全部满足 → 标记 Completed   │
│     - ⚠️ 部分满足 → 继续执行         │
│     - ❌ 无法满足 → 标记 Failed      │
└─────────────────────────────────────┘
```

### 6.3 自评估 Prompt 模板

当 Agent 需要进行自评估时，系统提示词中注入以下内容：

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【🎯 目标驱动模式】

当前活跃目标: {goal_name}
完成标准:
{criteria_list}

请对当前进度进行自我评估：
1. 哪些完成标准已满足？列出证据。
2. 哪些完成标准尚未满足？列出差距。
3. 下一步应该做什么？
4. 【如果所有标准都已满足】输出 "/goal complete" 标记完成。
5. 【如果判断无法完成】输出 "/goal fail <原因>" 标记失败。
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

---

## 7. 持久化执行循环

### 7.1 主循环增强

在现有 Agent 主循环基础上，增加 Goal-Driven 模式：

```rust
// 伪代码示意
pub async fn run(&mut self) -> anyhow::Result<()> {
    // ... 现有初始化代码 ...

    loop {
        // === 新增：Goal-Driven 模式检测 ===
        if let Some(active_goal) = self.goal_manager.active_goal() {
            if active_goal.status == GoalStatus::Active {
                // 注入 Goal 上下文到系统提示词
                self.context_manager.inject_goal_context(&active_goal);
                
                // 如果刚刚完成所有步骤，触发自评估
                if active_goal.all_steps_done() {
                    self.context_manager.inject_self_check_prompt();
                }
            }
        }

        // === 现有循环逻辑 ===
        // 读取输入 → 调用 LLM → 执行工具 → 收集结果
        
        // === 新增：检测 Goal 完成信号 ===
        // 检查 LLM 输出中是否包含 /goal complete 或 /goal fail 命令
        if let Some(command) = detect_goal_command(&final_assistant_message) {
            match command {
                GoalCommand::Complete => {
                    self.goal_manager.complete_goal()?;
                    // 输出完成总结
                    // 退出 Goal 模式，回到普通对话模式
                    is_auto = false;
                }
                GoalCommand::Fail(reason) => {
                    self.goal_manager.fail_goal(&reason)?;
                    // 分析失败原因，决定是否重试
                }
                GoalCommand::Cancel => {
                    self.goal_manager.cancel_goal()?;
                    is_auto = false;
                }
            }
        }
    }
}
```

### 7.2 三种执行模式

Agent 在运行时处于三种模式之一：

| 模式 | 说明 | is_auto | Goal 活跃 |
|------|------|---------|-----------|
| **对话模式** | 普通聊天，等待用户输入 | false | 无 |
| **目标驱动模式** | 自主执行直到 Goal 完成 | true | 有 |
| **子任务模式** | --task 模式，执行完退出 | true | 无（子 Agent） |

### 7.3 防无限循环机制

为防止 Agent 陷入无限循环，需要以下保护：

1. **最大轮次限制**：每个 Goal 最多执行 N 轮（默认 100）
2. **停滞检测**：连续 M 轮没有实质性进展（步骤数未增加）时，自动触发自评估
3. **重复操作检测**：检测到重复执行相同操作时，调整策略
4. **用户中断**：用户输入任意内容（或在交互模式下按 Ctrl+C）可打断

---

## 8. 与现有系统的集成

### 8.1 需要新增的文件

```
src/
├── goal/
│   ├── mod.rs              # 模块入口，重新导出
│   ├── types.rs            # Goal, GoalStatus 等数据类型
│   ├── registry.rs         # GoalRegistry — 持久化存储
│   ├── engine.rs           # GoalEngine — 执行逻辑
│   └── evaluator.rs        # GoalEvaluator — 自评估逻辑
```

### 8.2 需要修改的文件

| 文件 | 修改内容 |
|------|---------|
| `src/lib.rs` | 添加 `pub mod goal;` |
| `src/agent.rs` | 添加 GoalManager 字段，主循环中注入 Goal 上下文、检测 Goal 命令 |
| `src/task/types.rs` | 可选：添加 Goal 相关的 TaskState 字段 |
| `docs/index.md` | 添加 Goal 相关文档链接 |

### 8.3 与 TaskManager 的协作

```
Agent 主循环
    │
    ├── 用户设定 Goal
    │   ├── GoalManager::create_goal("实现登录功能", criteria)
    │   └── TaskManager::on_user_input("实现登录功能")
    │
    ├── 执行步骤
    │   ├── TaskManager: 更新 PLAN.md 步骤状态
    │   └── GoalManager: 更新 progress
    │
    ├── 自评估
    │   └── GoalEvaluator::evaluate(goal, context)
    │
    └── Goal 完成
        ├── GoalManager::complete_goal()
        └── 输出总结报告
```

---

## 9. 实现计划

### 阶段一：基础框架（P0）

1. **创建 Goal 数据类型** — `src/goal/types.rs`
   - Goal 结构体、GoalStatus 枚举、序列化/反序列化
2. **创建 GoalRegistry** — `src/goal/registry.rs`
   - 持久化到 `docs/goals/` 目录
   - 创建、读取、更新、列出 Goal
3. **注册到 lib.rs** — 添加 `pub mod goal;`
4. **编译验证** — `cargo check` 通过

### 阶段二：Agent 集成（P1）

1. **Agent 添加 GoalManager** — `src/agent.rs`
   - 初始化 GoalManager
   - 主循环中检测 Goal 命令（`/goal complete` 等）
2. **Goal 上下文注入** — 活跃 Goal 时注入系统提示
3. **自评估 Prompt** — 完成所有步骤后触发自评估
4. **Goal 完成/失败处理** — 状态更新 + 输出总结
5. **编译验证** — `cargo check` 通过

### 阶段三：测试与完善（P2）

1. **防无限循环机制** — 最大轮次、停滞检测
2. **跨会话恢复** — 启动时从 `docs/goals/` 恢复 Goal 状态
3. **用户中断处理** — 允许用户打断 Goal 执行
4. **文档更新** — 更新 docs/index.md，编写使用指南
5. **端到端测试** — spawn_agent 验证 Goal 驱动流程

### 阶段四：进阶功能（P3）

1. **多 Goal 管理** — 支持多个 Goal 排队/切换
2. **Goal 依赖** — Goal B 依赖 Goal A 的完成
3. **进度通知** — 进度变化时通知用户
4. **自动重试策略** — Failed 后自动重试（带方案调整）

---

## 10. 使用示例

### 10.1 用户设定 Goal

```
> 我的目标是：实现一个用户登录功能，包括前端登录表单和后端 API。

Agent:
🎯 已记录目标：实现用户登录功能
📋 完成标准：
  1. 后端 /api/login 接口可用
  2. 前端登录表单可提交
  3. 密码使用 bcrypt 加密
  4. JWT Token 返回给前端
  5. cargo check 编译通过

🚀 开始执行...
  [Step 1/5] 分析现有项目结构...
  [Step 2/5] 实现后端登录 API...
  ...
```

### 10.2 自评估完成

```
  [Step 5/5] 验证编译通过...

━━━ 自评估 ━━━
✅ /api/login 接口已实现 - 通过路由测试
✅ 前端登录表单已创建 - 包含用户名/密码输入
✅ bcrypt 加密已使用 - cargo add bcrypt
✅ JWT Token 已返回 - 使用 jsonwebtoken
✅ cargo check 通过 - 无错误

🎉 目标「实现用户登录功能」已完成！
📊 总结：
  - 创建文件: 3 个
  - 修改文件: 2 个
  - 耗时: 12 轮
```

### 10.3 目标失败与恢复

```
Agent:
❌ 步骤3 失败：第三方 API 服务不可用
📋 评估：当前无法完成，建议等待服务恢复
🔄 策略：切换到 Mock 模式继续开发，等 API 可用后再替换

... 切换到 Mock 模式后继续 ...

✅ 目标完成（使用 Mock 适配器）
⚠️ 注意：xxx API 就绪后需要替换 Mock
```

### 10.4 用户中断

```
> (用户按 Ctrl+C 或输入 "取消当前目标")

Agent:
⏸️ 目标「实现用户登录功能」已暂停
📊 当前进度：60%（3/5 步骤完成）
💡 使用 /goal resume 可继续，/goal cancel 取消
```

---

## 附录

### A. 与现有系统提示词的集成

当前的系统提示词在 `agent.rs` 中用 `format!` 构建。Goal 相关内容应作为**可选注入块**：

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【🎯 目标驱动模式 — 仅在活跃 Goal 时注入】

当前活跃目标: {goal_name}
完成标准:
{criteria_list}

当前进度: {progress}%

【自评估指令】
当你完成所有步骤后，请逐条检查完成标准。
如果全部满足，输出 "/goal complete" 标记完成。
如果判断无法完成，输出 "/goal fail <原因>"。
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### B. 关键设计决策记录

| 决策 | 选项 | 选择 | 理由 |
|------|------|------|------|
| Goal 存储格式 | JSON / Markdown | **JSON** | 结构化数据易于程序读写，不需要解析 Markdown |
| Goal 与 PLAN 的关系 | 合并 / 互补 | **互补** | PLAN.md 已成熟，无需替代，Goal 提供更高层抽象 |
| 自评估执行者 | LLM / 代码 | **LLM** | LLM 理解完成标准更灵活，代码只提供框架 |
| Goal 命令格式 | `/goal` 前缀 | **`/goal`** | 与现有 `/session`、`/debug` 风格一致 |
| 持久化目录 | `docs/goals/` | **`docs/goals/`** | 与现有 docs/ 体系一致，便于版本控制 |

### C. 参考实现

参考现有代码模式：

- **TaskManager** (`src/task/mod.rs`)：持久化状态管理，文件读写模式
- **SessionManager** (`src/session/mod.rs`)：会话的保存/加载模式
- **ErrorSnapshotManager** (`src/investigate/mod.rs`)：快照捕获/恢复模式
- **DebugTool** (`src/tools/tool_debug/mod.rs`)：简单状态管理与命令处理
