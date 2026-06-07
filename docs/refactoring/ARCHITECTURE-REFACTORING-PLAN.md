# 项目结构重构计划

> **版本**: v1.0  
> **日期**: 2025-06-08  
> **状态**: 📋 规划完成，待执行  
> **目标**: 解决当前项目结构混乱问题，建立清晰一致的目录规范

---

## 目录

1. [现状问题清单](#1-现状问题清单)
2. [目标目录结构](#2-目标目录结构)
3. [执行步骤](#3-执行步骤)
4. [验证标准](#4-验证标准)
5. [附录：变更明细](#5-附录变更明细)

---

## 1. 现状问题清单

### 🔴 P0 — 必须解决

| # | 问题 | 位置 | 描述 |
|---|------|------|------|
| 1 | **状态文件分裂** | `./PLAN.md` + `./docs/AGENDA.md` + `./docs/MEMORY.md` | TaskManager 同时从根目录和 docs/ 读取状态文件，位置不一致。根目录 `PLAN.md`（20KB）和 `docs/PLAN.md`（1.8KB）内容不同步，存在两份 |
| 2 | **空文件占位** | `src/agent.rs` (0行), `src/renderer/` (空目录) | 声明了但从未实现，给新开发者造成困惑 |
| 3 | **备份残留** | `src/main.rs.bak` (801行) | 备份文件混在源码中，不应提交 |

### 🟡 P1 — 应该解决

| # | 问题 | 位置 | 描述 |
|---|------|------|------|
| 4 | **超大源文件** | `src/context/mod.rs` (1291行), `src/context/strategy.rs` (1249行) | 单文件过长，职责边界模糊。`mod.rs` 包含 ContextManager + compress + preserve 等多种职责 |
| 5 | **`session/mod.rs` 过大** | `src/session/mod.rs` (613行) | 包含序列化类型、SessionManager、CLI 处理等多重职责 |
| 6 | **`main.rs` 过于臃肿** | `src/main.rs` (801行) | 主循环 + 渲染 + 初始化 + 工具调用可视化全部内联，不利于单元测试 |
| 7 | **Tool 目录命名不一致** | `src/tools/` | `edit_tool/`, `read_tool/`, `search_tool/`, `debug_tool/` 带 `_tool` 后缀，但 `base_shell/`, `subagent/` 不带 |
| 8 | **文档结构松散** | `docs/` | 文档平铺在 docs/ 下，feature-* 与分析文档混放，无分类索引 |

### 🟢 P2 — 可选优化

| # | 问题 | 位置 | 描述 |
|---|------|------|------|
| 9 | **commands/ 模块命名** | `src/commands/` | 模块名与内容匹配，但和外部 `command` 概念可能有歧义（建议改 `cli/`） |
| 10 | **无 docs/index.md** | `docs/` | 缺少文档索引入口，新人不清楚从哪开始阅读 |

---

## 2. 目标目录结构

```
agent-lab/
├── Cargo.toml
├── Cargo.lock
├── .env
├── .gitignore
├── README.md
│
├── docs/                              # 📚 所有文档
│   ├── index.md                       #    文档入口（WHAT / WHY / HOW）
│   ├── ARCHITECTURE.md                #    架构总览
│   ├── PLAN.md                        #    ✅ 状态文件统一放 docs/
│   ├── AGENDA.md                      #    ✅ 状态文件统一放 docs/
│   ├── MEMORY.md                      #    ✅ 状态文件统一放 docs/
│   ├── guides/                        #    使用指南
│   │   ├── getting-started.md
│   │   └── custom-tools.md
│   ├── designs/                       #    设计文档（原 feature-* 系列）
│   │   ├── context-window.md
│   │   ├── tool-call-visibility.md
│   │   └── agent-capability-roadmap.md
│   ├── analyses/                      #    分析文档
│   │   └── context-management-analysis.md
│   └── refactoring/                   #    重构计划
│       └── ARCHITECTURE-REFACTORING-PLAN.md   ← 本文档
│
├── src/
│   ├── main.rs                        # ✅ 入口（精简：CLI 解析 + 启动 agent）
│   │
│   ├── lib.rs                         # 🆕 库入口（方便单元测试+模块可见性控制）
│   │
│   ├── agent.rs                       # 🆕 Agent 核心循环（从 main.rs 提取）
│   │
│   ├── cli/                           # 📦 原 commands/ 重命名
│   │   ├── mod.rs
│   │   └── registry.rs
│   │
│   ├── model/                         # ✅ 模型适配器（保持不变）
│   │   ├── mod.rs
│   │   ├── types.rs
│   │   └── openai_compatible.rs
│   │
│   ├── context/                       # 🔧 上下文管理（需要拆分）
│   │   ├── mod.rs                     #    ✅ 只留 ContextManager + 公开 API
│   │   ├── manager.rs                 #    🆕 从 mod.rs 拆出 ContextManager 实现
│   │   ├── types.rs                   #    ✅ ContextMessage, CompressResult, Stats
│   │   ├── config.rs                  #    ✅ ContextStrategy 配置
│   │   ├── strategy.rs                #    🔧 只保留压缩策略逻辑
│   │   ├── summarizer.rs              #    ✅ 摘要生成器
│   │   └── tokenizer.rs               #    ✅ Token 估算器
│   │
│   ├── tools/                         # 🔧 统一命名（去掉 _tool 后缀）
│   │   ├── mod.rs                     #    ✅ ToolManager + ToolInfo
│   │   ├── types.rs                   #    ✅ Tool trait, ToolEvent, ToolStream
│   │   ├── shell/                     #    📦 原 base_shell/
│   │   ├── edit/                      #    📦 原 edit_tool/
│   │   ├── read/                      #    📦 原 read_tool/
│   │   ├── search/                    #    📦 原 search_tool/
│   │   ├── debug/                     #    📦 原 debug_tool/
│   │   └── subagent/                  #    ✅ 保持不变
│   │
│   ├── task/                          # ✅ 任务管理（保持不变）
│   │   ├── mod.rs
│   │   └── types.rs
│   │
│   ├── session/                       # 🔧 拆分（序列化类型独立）
│   │   ├── mod.rs                     #    ✅ SessionManager
│   │   └── types.rs                   #    🆕 SerializableMessage, SessionData（从 mod.rs 拆出）
│   │
│   ├── debug/                         # ✅ 全局 Debug 标志（保持不变）
│   │   └── mod.rs
│   │
│   └── renderer/                      # 🔧 保留但实现入口
│       ├── mod.rs                     #    🆕 实现基本渲染 trait
│       └── tool_result.rs             #    🆕 工具结果渲染逻辑（从 main.rs 提取）
│
├── .sessions/                         # ✅ 会话数据存储（gitignored）
│
└── 待删除：
    ├── AGENDA.md                      # ❌ 迁移到 docs/AGENDA.md
    ├── MEMORY.md                      # ❌ 迁移到 docs/MEMORY.md
    ├── PLAN.md                        # ❌ 内容合并到 docs/PLAN.md
    └── src/main.rs.bak                # ❌ 删除
```

### 文件映射表

| 当前路径 | 目标路径 | 操作 |
|----------|---------|------|
| `./PLAN.md` | `docs/PLAN.md` | 合并内容 |
| `./AGENDA.md` | `docs/AGENDA.md` | 合并内容 |
| `./MEMORY.md` | `docs/MEMORY.md` | 合并内容 |
| `./src/main.rs.bak` | — | 删除 |
| `./src/agent.rs` (0行) | `./src/agent.rs` (实现) | 从 main.rs 提取循环 |
| `./src/commands/` | `./src/cli/` | 重命名 |
| `./src/tools/base_shell/` | `./src/tools/shell/` | 重命名 |
| `./src/tools/edit_tool/` | `./src/tools/edit/` | 重命名 |
| `./src/tools/read_tool/` | `./src/tools/read/` | 重命名 |
| `./src/tools/search_tool/` | `./src/tools/search/` | 重命名 |
| `./src/tools/debug_tool/` | `./src/tools/debug/` | 重命名 |
| `docs/feature-*.md` | `docs/designs/` | 移动 |
| `docs/context-management-analysis.md` | `docs/analyses/` | 移动 |

---

## 3. 执行步骤

### 阶段 0：清理与准备

- [ ] **0.1** 合并根目录和 docs/ 的状态文件（PLAN.md, AGENDA.md, MEMORY.md）
  - 将根目录的三个文件内容合并到 docs/ 下对应文件
  - 更新 `src/task/mod.rs` 中的 `STATE_FILES` 路径，全部指向 `docs/`
  - 删除根目录的三个状态文件
  - **验证**: `cargo check && task 模块正确读取 docs/ 下的状态文件`

- [ ] **0.2** 删除无用文件
  - 删除 `src/main.rs.bak`
  - 清理空的 `src/renderer/` 目录（如果暂时不用）
  - **验证**: 无编译错误

### 阶段 1：模块重命名（分批进行，每批验证）

- [ ] **1.1** 工具目录统一命名（去掉 `_tool` 后缀）
  - `base_shell/` → `shell/`
  - `edit_tool/` → `edit/`
  - `read_tool/` → `read/`
  - `search_tool/` → `search/`
  - `debug_tool/` → `debug/`（注意与 `src/debug/` 冲突！→ 改为 `tool_debug/` 或合并到 `src/debug/`）
  - 更新 `src/tools/mod.rs` 的 `mod` 声明
  - 更新 `src/main.rs` 中的导入路径
  - **验证**: `cargo check`

- [ ] **1.2** `commands/` → `cli/` 重命名
  - 移动 `src/commands/` → `src/cli/`
  - 更新 `src/main.rs` 的 `mod commands;` → `mod cli;`
  - **验证**: `cargo check`

### 阶段 2：大文件拆分

- [ ] **2.1** 拆分 `src/context/mod.rs`（1291行）
  - 将 ContextManager 的核心实现移到 `manager.rs`
  - `mod.rs` 只保留 re-export 和公开 API
  - **验证**: `cargo check`

- [ ] **2.2** 拆分 `src/session/mod.rs`（613行）
  - 将 `SerializableMessage`、`SerializableToolCall`、`SessionData` 移到 `types.rs`
  - **验证**: `cargo check`

### 阶段 3：文档重组

- [ ] **3.1** 创建 docs/ 子目录结构
  - 创建 `docs/guides/`、`docs/designs/`、`docs/analyses/`、`docs/refactoring/`
  - 将 `feature-*.md` 移到 `docs/designs/`
  - 将 `context-management-analysis.md` 移到 `docs/analyses/`
  - 将本文档放入 `docs/refactoring/`
  - **验证**: 所有文档可找到，README.md 中的链接更新

- [ ] **3.2** 创建 `docs/index.md` 文档索引
  - 汇总所有文档的分类和用途
  - 提供导航指引
  - **验证**: 索引完整且所有链接有效

### 阶段 4：架构优化（可选）

- [ ] **4.1** 创建 `src/lib.rs` 库入口
  - 将 `main.rs` 中的公共模块声明移到 `lib.rs`
  - `main.rs` 只保留 `fn main()` + 导入 `agent_lab::*`
  - 启用集成测试
  - **验证**: `cargo test` 通过

- [ ] **4.2** 从 `main.rs` 提取 Agent 核心循环到 `agent.rs`
  - 提取 `main.rs` 中的主事件循环
  - 定义 `Agent` struct 封装状态（ContextManager, ToolManager, ModelAdapter 等）
  - **验证**: `cargo check`

---

## 4. 验证标准

| 阶段 | 验证方式 | 通过条件 |
|------|---------|---------|
| 0.1 | `cargo check` | 编译通过 |
| 0.1 | 读取状态文件 | TaskManager 能从 `docs/` 正确读取状态 |
| 0.2 | `ls src/main.rs.bak` | 文件不存在 |
| 1.x | `cargo check` | 编译通过 |
| 1.x | `cargo run -- --help` | 入口正常工作 |
| 2.x | `cargo check` | 编译通过 |
| 2.x | 文件行数 | context/mod.rs < 200行, session/mod.rs < 200行 |
| 3.x | 文档链接 | 所有交叉引用有效 |
| 4.x | `cargo test` | 所有测试通过 |
| 4.x | `cargo run -- --help` | 二进制正常工作 |

---

## 5. 附录：变更明细

### 5.1 文件操作总表

| 操作 | 路径 | 说明 |
|------|------|------|
| 🗑️ 删除 | `./PLAN.md` | 内容合并到 `docs/PLAN.md` |
| 🗑️ 删除 | `./AGENDA.md` | 内容合并到 `docs/AGENDA.md` |
| 🗑️ 删除 | `./MEMORY.md` | 内容合并到 `docs/MEMORY.md` |
| 🗑️ 删除 | `./src/main.rs.bak` | 无用的备份 |
| 📦 重命名 | `src/commands/` → `src/cli/` | 更清晰的命名 |
| 📦 重命名 | `src/tools/base_shell/` → `src/tools/shell/` | 去掉无意义前缀 |
| 📦 重命名 | `src/tools/edit_tool/` → `src/tools/edit/` | 去掉 `_tool` 后缀 |
| 📦 重命名 | `src/tools/read_tool/` → `src/tools/read/` | 同上 |
| 📦 重命名 | `src/tools/search_tool/` → `src/tools/search/` | 同上 |
| 📦 重命名 | `src/tools/debug_tool/` → `src/tools/tool_debug/` | 避免与 `src/debug/` 冲突 |
| ✂️ 拆分 | `src/context/mod.rs` → `manager.rs` | 拆分大文件 |
| ✂️ 拆分 | `src/session/mod.rs` → `types.rs` | 拆分大文件 |
| 🆕 新增 | `src/lib.rs` | 库入口 |
| 🆕 新增 | `docs/index.md` | 文档索引 |
| 🆕 新增 | `docs/refactoring/` | 重构计划目录 |
| 🚚 移动 | `docs/feature-*.md` → `docs/designs/` | 归类 |
| 🚚 移动 | `docs/context-management-analysis.md` → `docs/analyses/` | 归类 |

### 5.2 关键代码修改

1. **`src/task/mod.rs`** — `STATE_FILES` 常量改为全部指向 `docs/` 目录
2. **`src/tools/mod.rs`** — 更新所有 `mod` 声明对应的目录名
3. **`src/main.rs`** — 更新所有导入路径
4. **`src/context/mod.rs`** — 拆分后只保留 re-export
5. **`src/session/mod.rs`** — 拆分后只保留 SessionManager

---

## 6. 执行顺序策略

```
阶段 0（清理） → 阶段 1（重命名） → 阶段 2（拆分） → 阶段 3（文档） → 阶段 4（架构）
     ↓               ↓                  ↓               ↓               ↓
  快速见效       不影响逻辑          降低认知          易维护          可测试
  低风险         中风险              中风险            低风险          高风险
```

> **建议**: 按阶段顺序执行，每完成一个子步骤都运行 `cargo check` 验证。  
> **回滚策略**: 如果某个步骤导致复杂问题，整体回退到上一个已验证的 commit。
