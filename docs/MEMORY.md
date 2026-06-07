# MEMORY.md — 重要记录

## 2024-06-07: 修复上下文自动压缩不生效

### 问题描述
上下文使用率达到 100% 时，压缩机制没有被触发。

### 根因分析
`auto_compress` 中的滑动窗口（层1）触发条件仅依赖轮数（`turns > max_turns`，默认 20 轮）。

当 token 使用率在 `trigger_threshold`（70%）到 `token_limit`（100%）之间时：
1. **层0（工具修剪）**：如果工具结果本身不长（<= 200 chars）或没有工具结果 → 不生效
2. **层1（滑动窗口）**：如果轮数 <= 20 → 不触发
3. **层3（保底截断）**：如果 token <= 128000 → 不触发

最终返回 `NotNeeded`，没有任何压缩发生。

### 修复方案
在 `auto_compress` 的情况 C 中，增加基于 token 数的动态 `effective_max_turns` 计算：

- 当 `current_tokens >= trigger_threshold` 时，按超出比例降低有效 max_turns
- 公式：`effective_max_turns = max_turns * (trigger_threshold / current_tokens)`
- 举例：token 100%（128000）→ effective = 20 * 0.7 = 14，15轮时触发压缩
- 安全边界：`max(1, min(effective, max_turns))`

### 修改文件
- `src/context/strategy.rs`：`auto_compress` 函数中的情况 C（第396-432行）

### 验证
- `cargo check` 通过
- `cargo test` 全部 64 个测试通过


## 2024-06-07: 结构化任务执行框架（TaskManager）

### 问题描述
Agent 在扁平循环中缺乏任务意识——上下文压缩后丢失进度，没有自动的任务状态持久化机制。

### 设计决策
引入语言无关的 TaskManager，核心设计原则：
1. **代码+文件双重状态管理** — TaskManager 维护内存中的结构化状态，同时读写 PLAN.md / AGENDA.md / MEMORY.md 作为持久化
2. **压缩感知注入** — 上下文压缩后自动注入当前任务状态，帮助模型恢复上下文
3. **去重保护** — 状态未变化时不重复注入，避免上下文膨胀
4. **Agent 可编辑** — 状态文件是 Markdown 格式，Agent 可通过 edit 工具直接修改，TaskManager 也支持通过 load() 读取

### 修改文件
| 文件 | 操作 |
|------|------|
| `src/task/mod.rs` | 新增 — TaskManager 结构体 + 文件读写 + 状态注入 |
| `src/task/types.rs` | 新增 — TaskState 数据类型 + 序列化 |
| `src/main.rs` | 修改 — 注册 task 模块，主循环中集成 TaskManager |
| `Cargo.toml` | 修改 — 添加 tempfile 作为 dev-dependency |

### 验证
- `cargo check` 通过
- `cargo test` 75 passed（11 个新测试 + 64 个原有测试）


## 2024-06-07: 验证上下文压缩能力 — 综合验证

### 验证范围
对 ContextManager 的四层渐进式压缩进行了全面验证：

| 测试 | 文件 | 覆盖内容 |
|------|------|----------|
| `test_token_cache_incremental_vs_full_consistency` | `mod.rs` | Token 缓存增量更新 vs 全量重算一致性（10条消息逐条验证） |
| `test_dynamic_max_turns_triggers_compression_early` | `mod.rs` | token-based 动态 max_turns 触发（8轮触发20轮上限） |
| `test_inject_summary_reduces_tokens` | `mod.rs` | 异步摘要注入后 token 真实下降 + 消息数减少 |
| `test_end_to_end_compression_lifecycle` | `mod.rs` | 全生命周期：20轮对话 → 压缩触发 → token受控 → system保留 |
| `test_progressive_order_tool_pruning_before_sliding_window` | `mod.rs` | 工具修剪(层0)保留消息结构 + 内容被占位符替换 |
| `test_token_based_trigger_with_few_turns` | `mod.rs` | 极端情况：2轮但token超阈值 → 触发压缩 |

### 验证结果
- 81 tests passed（75原有 + 6新增）
- `cargo check` 通过
- `spawn_agent` 编译成功并验证通过
- 所有验证标准已满足

### 关键发现
1. Token 增量缓存 100% 准确——与全量重算完全一致
2. 动态 max_turns 在 token 超阈值但轮次不足时正确触发（公式：`effective = max_turns * (trigger_threshold / current_tokens)`）
3. 摘要注入 + 删除原文后 token 显著下降（不是只增不减）
4. 端到端场景中压缩正确触发，token 被控制在 token_limit 附近
5. 工具修剪(层0)优先于滑动窗口(层1)，保留完整对话结构

## 2024-06-07: 🔴 Bug — auto_compress 在实战中永不触发

### 问题描述
上下文使用率在实战中总是达到 100%（128K tokens），压缩从不触发。所有单元测试通过，但真实 Agent 循环中无效。

### 根因分析

**`auto_compress` 函数存在三个连锁问题：**

#### 问题1: `effective_max_turns` 公式过于保守

当前公式：
```rust
let reduced = (max_turns as f64 * target_ratio).ceil() as usize;
// 其中 target_ratio = trigger_threshold / current_tokens
```

当 `current_tokens` 刚超过 `trigger_threshold`（70%=89.6K）时：
- 90K tokens → target_ratio = 89.6K/90K = 0.996 → reduced = ceil(20*0.996) = 20
- **effective_max_turns 仍然是 20！** 需要 21 轮才能触发滑动窗口

即使到 100K tokens：
- target_ratio = 89.6K/100K = 0.896 → reduced = ceil(20*0.896) = 18
- effective_max_turns = 18，仍然很高

**结果**: 在 tokens 从 70% 到 100% 的整个区间，effective_max_turns 下降极慢，导致滑动窗口几乎从不触发。

#### 问题2: `hard_truncate` 使用 `>` 而非 `>=`

```rust
if tokens_after > token_limit {  // 128000 > 128000 = false！
```

恰好 100% 时，硬截断也不会触发。

#### 问题3: 三层全跳过 → NotNeeded

当上述问题同时出现时：
- 层0（工具修剪）：如果工具输出都小于 200 chars，无修剪必要
- 层1（滑动窗口）：turns <= effective_max_turns，不触发
- 层3（保底截断）：tokens <= token_limit，不触发

→ 返回 `NotNeeded`，压缩永不生效。

### 典型命中场景

```
每轮对话：长思考链(8K) + 短工具结果(短) → turns ≈ 14 时 token ≈ 123K
effective_max_turns = 15, turns 15 > 15? false → 滑动窗口不触发
工具结果 < 200 chars → 修剪不触发
123K > 128K? false → 截断不触发
→ NotNeeded!!
```

### 修复方案

1. **`effective_max_turns` 改为线性缩放**
   公式：`max_turns * (1.0 - (current_tokens - trigger_threshold) / (token_limit - trigger_threshold))`
   - 70% tokens → 20 轮
   - 85% tokens → 10 轮
   - 100% tokens → 1 轮

2. **`hard_truncate` 触发条件改为 `>=`**
   确保 100% 时也能触发

3. **增加诊断日志**
   每次 auto_compress 运行时输出状态到 stderr

### 为什么单元测试没发现

单元测试构造的测试数据中 token 密度均匀，`make_context_messages_with_tools` 每轮包含长工具输出（>200 chars），因此层0总能在 token 超阈值时生效。但实战中可能存在「长思考 + 短工具结果」的模式，层0无效。

### 修改文件
- `src/context/strategy.rs`：修复 `auto_compress` 中的 effective_max_turns 公式 和 hard_truncate 触发条件

## 2024-06-XX: 🔴 Bug — 上下文 100% 后模型停止调用工具

### 用户反馈
每次 Token 使用率达到 100% 后，模型就不再继续调用工具了（auto-loop 停止）。

### 根因分析

#### 问题1: auto-loop 模式下没有 User 消息，滑动窗口误判轮数
`count_turns()` 通过统计 User 消息数量来计算轮次。但在 auto-loop 模式下：
- 只有一个初始的 User 消息（任务指令）
- 后续的 tool_calls + tool_results 全是 Assistant/Tool 消息
- 因此 `turns == 1` 始终成立
- 即使压缩层计算出 `effective_max_turns = 1`，滑动窗口的 `turns <= max_turns` 检查也会返回 `NotNeeded`

**结果**: 滑动窗口层在 auto-loop 模式下永远无法压缩，直接跳过。

#### 问题2: hard_truncate 可能无法有效降低 token
当滑动窗口跳过时，所有压缩压力都落到 hard_truncate 上。但如果大量消息被标记为 preserved/important，hard_truncate 能删除的消息有限。

#### 问题3: 没有前置阻塞检测
模型 API 在被调用时上下文可能已经超过 128K token，DeepSeek 等 API 会返回错误（如 context_length_exceeded）。错误被 `_ => ()` 静默忽略，导致模型无响应。

#### 问题4: 异步摘要可能来不及
异步摘要在滑动窗口前派发，但摘要结果注入需要等到下一次 loop 迭代。在此期间 token 持续积累。

### 修复方案（对应 docs/PLAN.md 中的 P0 计划）

1. ✅ **步骤1**: 添加 `ForceCompressed` 压缩结果类型 (已完成)
2. **步骤2**: 修改 `auto_compress` 添加 `force` 参数，跳过常规检查直接执行最激进压缩
3. **步骤3**: 添加 `is_blocked()` / `force_compress()` 方法到 ContextManager
4. **步骤4**: 在 `main.rs` 中调用模型前检查阻塞状态，阻塞时先强制压缩再发送
5. **步骤5**: 修复 auto-loop 下的轮次计算，让滑动窗口在无 User 消息时也能触发

### 修改文件
- `src/context/types.rs` — ForceCompressed 变体
- `src/context/strategy.rs` — auto_compress force 参数
- `src/context/mod.rs` — is_blocked / force_compress 方法
- `src/main.rs` — 调用模型前阻塞检查

## 2024-06-07: 修复记忆压缩后导致后续调用结束的 Bug

### 根本原因
1. **`hard_truncate` 破坏对话结构**：硬截断按消息逐个删除 unprotected 消息，从最早的开始。
   当删除 Assistant(tool_calls) 消息但保留后续 Tool 消息时，对话结构变得无效。
   Tool 消息没有对应的 Assistant tool_calls 前驱，导致 LLM API 调用失败。

2. **`ModelEvent::Error` 被静默忽略**：main.rs 使用 `_ => ()` 忽略了所有未显式处理的 ModelEvent，
   包括 Error。API 错误被吞掉后，`has_tool_calls` 保持 false，`is_auto` 被设为 false，
   在 `--task` 模式下触发 `break Ok(())` 提前退出。

### 修复内容

#### 1. `src/context/strategy.rs` - hard_truncate 修复孤儿 Tool 消息
- 在构建新消息列表后，扫描并删除"孤儿"Tool 消息
- 实现方式：先收集所有 Assistant tool_calls 中的活跃 tool_call_id，
  然后从后往前删除不在活跃列表中的 Tool 消息
- 添加诊断日志：`[hard_truncate] removed N messages (including M orphaned tool results)`

#### 2. `src/main.rs` - ModelEvent::Error 日志
- 将 `_ => ()` 拆分为 `ModelEvent::Error(err) => { eprintln!("❌ 模型 API 错误: {}", err); }`
- 错误现在会输出到 stderr，agent 能感知到 API 错误

### 验证
- `cargo check` 通过
- `cargo test` 90 passed


## 2025-06-08: 项目结构重构规划完成

### 任务描述
应要求输出项目结构重构计划，分析当前结构混乱问题并提出系统化方案。

### 发现的现状问题（10个）

| 优先级 | 问题 | 描述 |
|--------|------|------|
| 🔴 P0 | 状态文件分裂 | `./PLAN.md` + `./docs/AGENDA.md` + `./docs/MEMORY.md` 不一致，TaskManager 读取策略混乱 |
| 🔴 P0 | 空文件占位 | `src/agent.rs` (0行), `src/renderer/` (空目录) |
| 🔴 P0 | 备份残留 | `src/main.rs.bak` |
| 🟡 P1 | 超大源文件 | `context/mod.rs` (1291行), `context/strategy.rs` (1249行) |
| 🟡 P1 | session/mod.rs过大 | 613行包含序列化+业务+CLI多重职责 |
| 🟡 P1 | main.rs臃肿 | 801行，循环+渲染+初始化全内联 |
| 🟡 P1 | Tool命名不一致 | `edit_tool/`, `read_tool/` vs `base_shell/`, `subagent/` |
| 🟡 P1 | 文档结构松散 | feature-* 与分析文档混放 |
| 🟢 P2 | commands/命名 | 建议改为 cli/ |
| 🟢 P2 | 无文档索引 | 缺少 docs/index.md |

### 输出文档
- `docs/ARCHITECTURE-REFACTORING-PLAN.md` — 完整重构计划（6阶段、10问题、文件映射表、验证标准）


## 📦 归档：来自旧根目录状态文件的历史记录

> 以下内容来自原根目录 `MEMORY.md`，重构后统一归入 `docs/MEMORY.md`

### 旧记录：清理死代码 `maybe_dispatch_summary`
- 异步摘要派发逻辑已从 `check_and_compress` 集成到 `auto_compress` 内部（作为层1），旧的 `maybe_dispatch_summary` 方法成为死代码
- **操作**：删除 `src/context/mod.rs` 中的 `maybe_dispatch_summary` 方法

### 旧记录：修复策略.rs 测试编译错误
- `auto_compress` 函数签名增加了第5个参数 `summary_tx: Option<mpsc::UnboundedSender<SummaryTask>>`，但测试中的辅助函数未更新

### 旧记录：修复 sanitize_name 测试失败
- `sanitize_name` 函数未区分空格（应替换为 `_`）和其他特殊字符（应移除）
- **修复**：空格→下划线，其他特殊字符→空格→过滤移除

### 旧记录：`/` 命令系统
- 创建 `src/commands/mod.rs`：Command + CommandRegistry
- 内置命令：help, clear, session, sessions, tools
- 5 个单元测试

### 旧记录：压缩后孤立 Tool 消息清理
- `remove_orphaned_tool_messages()` 覆盖全部5个压缩路径
- 验证：cargo test 94 passed


## 2025-06-08: 项目结构重构执行进度

### 已完成 ✅

**阶段 0：清理与准备**
- ✅ 合并根目录和 docs/ 的状态文件（PLAN.md, AGENDA.md, MEMORY.md → docs/）
- ✅ 更新 `src/task/mod.rs` 的 STATE_FILES 和所有文件路径（3处代码 + 2处测试）
- ✅ 删除根目录旧状态文件和 `src/main.rs.bak`
- ✅ 删除空 `src/renderer/` 目录
- 验证：cargo check ✅ | 94 tests passed ✅

**阶段 1：模块重命名**
- ✅ 工具目录统一命名（6个目录重命名）
  - `base_shell/` → `shell/`
  - `edit_tool/` → `edit/`
  - `read_tool/` → `read/`
  - `search_tool/` → `search/`
  - `debug_tool/` → `tool_debug/`
  - 注：`subagent/` 保持原名（无 _tool 后缀）
- ✅ `commands/` → `cli/`（模块名 + 目录 + 所有引用更新）
- 验证：cargo check ✅ | 94 tests passed ✅

### 待执行 ⬜

**阶段 2：大文件拆分**
- [ ] 2.1 拆分 context/mod.rs（1291行，21个pub方法）
- [ ] 2.2 拆分 session/mod.rs（613行）

**阶段 3：文档重组**
- [ ] 3.1 创建 docs/ 子目录结构
- [ ] 3.2 创建 docs/index.md 文档索引

**阶段 4：架构优化**
- [ ] 4.1 创建 src/lib.rs 库入口
- [ ] 4.2 提取 Agent 核心循环到 agent.rs


## 2024-07-04: 大文件拆分完成

### 完成的工作

**context/mod.rs (1291 → 449 行)**
- 将 `#[cfg(test)] mod tests` (843行) 拆分到 `src/context/tests.rs`
- `context/mod.rs` 只保留核心的 `ContextManager` 结构体和方法的实现

**session/mod.rs (613 → 369 行)**
- 将类型定义（SerializableMessage, SessionData, SerializedContextMessage, 等 + From impls, 127行）拆分到 `src/session/types.rs`
- 将测试模块（120行）拆分到 `src/session/tests.rs`
- `session/mod.rs` 只保留 `SessionManager`, `SessionInfo` 和业务逻辑

### 验证结果
- `cargo check`: ✅ 编译通过
- `cargo test`: ✅ 全部 94 个测试通过
- 功能无变化，纯文件结构优化

## 2025-06-08: 项目结构重构完成（阶段 0-4.1）

### 完成情况

| 阶段 | 状态 | 内容 |
|------|------|------|
| 阶段 0：清理准备 | ✅ | 状态文件合并到 docs/，删除 main.rs.bak 和空目录 |
| 阶段 1：模块重命名 | ✅ | tools/*_tool/ → tools/*/，commands/ → cli/ |
| 阶段 2：大文件拆分 | ✅ | context/mod.rs 拆出 tests.rs，session/mod.rs 拆出 types.rs + tests.rs |
| 阶段 3：文档重组 | ✅ | docs/ 子目录化（designs/、analyses/、refactoring/、guides/）+ index.md |
| 阶段 4.1：库入口 | ✅ | 创建 src/lib.rs，main.rs 使用 `use agent_lab::` 导入 |

### 未完成
- **阶段 4.2**: 从 main.rs 提取 Agent 核心循环到 agent.rs（可选，待后续）

### 验证
- `cargo check`: ✅ 编译通过
- `cargo test`: ✅ 全部 94 个测试通过
- 文档结构清晰，状态文件统一在 docs/

## 2024-06-07: 🔧 修复编译错误 — SpawnAgent Send + borrow after move

### 问题描述
cargo check 报三个编译错误：
1. `SpawnAgent` 中 `Box<dyn ModelAdapter>` 不满足 `Send` trait bound
2. `ChatMessage` 枚举的变体包含 `dyn ToolResultRenderer` 不满足 `Send`
3. `AgentBuilder::build()` 中 `self.config` 在 `to_strategy()` 调用后被 move

### 修复方案

**错误1: ChatMessage 不是 Send**
- 给 `ToolResultRenderer` trait 加上 `Send` bound
- 给 `ModelAdapter` trait 加上 `Send` bound
- 修改 `SpawnAgent` 字段类型为 `Arc<Mutex<Box<dyn ModelAdapter + Send>>>`
- 修改所有引用处适配 `Arc<Mutex<...>>` 模式（lock/unlock）

**错误2: SpawnAgent 的 model 字段类型**
- 同上，改为 `Arc<Mutex<Box<dyn ModelAdapter + Send>>>`

**错误3: AgentBuilder::build() borrow after move**
- 将 `self.config.to_strategy()` 提取到局部变量 `strategy`，在 move `self.config` 到 struct 之前调用

### 修改文件
- `src/tools/subagent/mod.rs` — `SpawnAgent` 字段类型 + ModelAdapter Send bound
- `src/context/mod.rs` — `ToolResultRenderer` Send bound
- `src/model/mod.rs` — `ModelAdapter` Send bound
- `src/tools/subagent/spawn.rs` — 适配 `Arc<Mutex<...>>`
- `src/agent.rs` — `build()` 中提前提取 strategy

### 验证
- `cargo check` 通过（0 errors, 19 warnings）


## 2025-06-08: 错误排查（Error Investigation）能力技术方案

### 设计决策（v2 简化版）
用户指出核心场景不是「完整回放」，而是「工具调用报错时记录现场并排查」。方案从三层回放架构简化为聚焦核心场景的轻量方案。

### 核心设计
1. **ErrorSnapshot** — 工具报错时自动保存「错误现场」（最后几轮消息 + 错误信息 + 任务状态），存到 `.agent/snapshots/*.json`
2. **InvestigateTool** — 新增 `investigate` 工具，加载快照 + 调用 LLM 做根因分析，输出排查报告
3. **spawn_agent 集成** — 子 agent 报错时输出 `[SNAPSHOT]` 标记，主 agent 自动触发排查

### 对比旧方案（v1 vs v2）
| 维度 | v1（三层回放） | v2（错误排查） |
|------|---------------|---------------|
| 记录范围 | 每步都记录 | 只在报错时记录 |
| 存储 | `.agent/replay/*.json` | `.agent/snapshots/*.json` |
| 工具 | `replay` 三种模式 | `investigate` 单一工具 |
| 单快照大小 | ~5KB/轮 | <10KB/每次报错 |
| 复杂度 | 高 | 低 |

### 尚未实现
- `src/investigate/` 模块代码尚未编写
- `InvestigateTool` 尚未实现
- spawn_agent 尚未集成
