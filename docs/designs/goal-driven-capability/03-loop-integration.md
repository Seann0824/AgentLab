# 🎯 目标驱动能力（Goal-Driven Capability） — 执行循环与系统集成

> 原文拆分自 `../goal-driven-capability.md`。

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

