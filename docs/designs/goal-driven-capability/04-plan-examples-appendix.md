# 🎯 目标驱动能力（Goal-Driven Capability） — 实现计划、示例与附录

> 原文拆分自 `../goal-driven-capability.md`。

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
