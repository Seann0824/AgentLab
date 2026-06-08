# 🎯 目标驱动能力（Goal-Driven Capability） — Goal 数据结构与生命周期

> 原文拆分自 `../goal-driven-capability.md`。

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

