# 📚 文档索引

> **Agent Lab — 自我进化的 AI Agent 框架**
>
> 本文档是项目文档的入口。所有文档按分类组织，方便快速定位。

---

## 📋 状态跟踪

| 文件 | 用途 | 最新更新 |
|------|------|---------|
| [PLAN.md](./PLAN.md) | 当前执行计划（步骤 + 完成状态） | ✅ 活跃 |
| [AGENDA.md](./AGENDA.md) | 当前议程（任务名 + 进度 + 当前步骤） | ✅ 活跃 |
| [MEMORY.md](./MEMORY.md) | 重要发现、关键决策、已知问题 | ✅ 活跃 |

---

## 🏗️ 架构与重构

| 文件 | 用途 |
|------|------|
| [重构计划](./refactoring/ARCHITECTURE-REFACTORING-PLAN.md) | 项目结构重构的完整计划与目标目录结构 |

---

## 🎨 设计文档

| 文件 | 用途 |
|------|------|
| [DAG 任务编排系统](./designs/dag-task-orchestration.md) | 基于有向无环图的任务编排，每个节点内置 Worker+Reviewer 双 Agent |
| [上下文窗口管理](./designs/context-window.md) | 四层渐进压缩策略的设计与实现 |
| [工具调用可见性](./designs/tool-call-visibility.md) | Agent 工具调用结果的可视化设计 |
| [能力路线图](./designs/agent-capability-roadmap.md) | 项目能力规划与未来方向 |

---

## 🔬 分析文档

| 文件 | 用途 |
|------|------|
| [上下文管理分析](./analyses/context-management-analysis.md) | 上下文管理方案的深度分析 |

---

## 📖 使用指南

> 指南文档尚未创建。以下主题待补充：
>
> - **快速入门** — 如何配置和运行 Agent Lab
> - **自定义工具** — 如何为 Agent 添加新工具
> - **会话管理** — 如何保存和恢复对话
> - **调试模式** — 如何启用调试日志

---

## 文件分类规则

| 分类 | 目录 | 内容约定 |
|------|------|---------|
| **状态跟踪** | `docs/` | 由 TaskManager 自动维护，Agent 可编辑 |
| **架构** | `docs/refactoring/` | 重构计划、架构决策记录 |
| **设计** | `docs/designs/` | 原 `feature-*` 系列设计文档 |
| **分析** | `docs/analyses/` | 深度技术分析文档 |
| **指南** | `docs/guides/` | 用户使用指南（待创建） |

---

> 最后更新: 2025-06-08
