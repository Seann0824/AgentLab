# 结构化任务执行框架（TaskManager）

## 目标
在 Agent 中引入结构化任务执行框架，让 Agent 能在多轮对话中保持任务状态，在上下文压缩后能自动恢复进度。

核心能力（语言无关）：
- 任务状态持久化（PLAN.md / AGENDA.md / MEMORY.md 由代码自动维护）
- 上下文压缩后自动注入当前任务状态
- 支持任务开始、步骤完成、重要发现等生命周期

## 执行步骤

- [x] 1. 创建 `src/task/mod.rs` — TaskManager 核心结构体 + 文件读写
- [x] 2. 创建 `src/task/types.rs` — 任务状态数据类型
- [x] 3. 在 `main.rs` 中注册 `task` 模块
- [x] 4. 修改 `main.rs` 主循环，在上下文压缩后注入任务状态
- [x] 5. 运行 `cargo check` 验证编译通过
- [x] 6. 运行 `cargo test` 确保不影响现有功能（75 passed）

## 验证标准
- [x] `cargo check` 通过
- [x] 所有现有测试通过（75 passed）
- [x] TaskManager 能正确读写状态文件（test_save_and_load_roundtrip + test_load_from_files）
- [x] 压缩后能生成有效的状态提示注入上下文（test_get_inject_message 系列）
# 修复 auto_compact 功能

## 目标
修复上下文自动压缩（auto_compact）功能，使四层渐进压缩模型真正工作：
层0（工具修剪）→ 层1（异步摘要）→ 层2（滑动窗口）→ 层3（保底截断）

## 问题分析
1. **摘要派发时机错误**：`maybe_dispatch_summary` 在滑动窗口之后调用，此时早期消息已被删除，摘要器无内容可处理
2. **`keep_recent` 过大**：摘要 scope 用 `max_turns=20` 作为 keep_recent，滑动窗口后剩余轮次 ≤20，筛不出任何消息
3. **摘要注入不删除原文**：`inject_summary` 只插入摘要消息，不删除被摘要的原始消息，导致 token 不降反升
4. **缺乏 Token 触发**：摘要只在滑动窗口后触发，无法在 Token 超阈值但轮次不足时独立触发

## 执行步骤

### Step 1: 修复 `maybe_dispatch_summary` 捕获压缩前快照
- [x] `check_and_compress` 中调用 auto_compress 前保存消息快照
- [x] 滑动窗口压缩后，用快照派发摘要任务（让摘要器能看到被删的消息）

### Step 2: 修复 `SummaryResult` 携带被摘要消息信息
- [x] 在 `SummaryResult` 中添加 `summarized_count: usize` 字段
- [x] `AsyncSummarizer` 返回被摘要的消息数量

### Step 3: 修复 `inject_summary` 删除被摘要的原始消息
- [x] 摘要注入时删除对应的原始消息（从系统提示词之后开始删除 summarized_count 条）
- [x] 更新 token 缓存（全量重算 cache）
- [x] 处理插入/删除后的索引偏移（drain 操作安全处理边界）

### Step 4: 让摘要能独立于滑动窗口触发
- [x] `compress()` 方法中也传入快照
- [x] auto_compress 在硬截断后也派发摘要（check_and_compress 中已处理 HardTruncated 派发）

### Step 5: 验证编译和测试
- [x] `cargo check` 通过
- [x] 现有测试通过（75 passed）
- [x] 新增 auto_compact 集成测试（test_agent_loop_simulation_with_compression）

## 验证标准
- [ ] cargo check 无错误
- [ ] 异步摘要真正产生并替换早期消息
- [ ] Token 计数在摘要注入后正确下降

# 新增子 Agent 验证能力（spawn_agent 工具）

## 目标
为 Agent 增加「自我迭代验证」能力：修改代码后能编译新版本 agent，并派生子 agent 进程执行指定任务，验证改动是否按预期工作。

## 设计思路
1. 新增 `spawn_agent` 工具，接受任务描述和超时参数
2. 工具内部：编译 agent → 启动子进程 → 写入任务 → 收集输出 → 返回结果
3. 修改 `main.rs` 支持 `--task` 参数（单次运行模式，完成后退出）
4. 主 agent 可通过 spawn_agent 验证自身修改的效果

## 执行步骤

- [x] 1. 创建 `src/tools/subagent/mod.rs` — spawn_agent 工具实现
- [x] 2. 在 `src/tools/mod.rs` 注册 spawn_agent 工具
- [x] 3. 修改 `main.rs` 支持 `--task` CLI 参数（单次运行模式）
- [x] 4. 更新系统提示词，告知 agent 可用 spawn_agent 工具（已存在于 main.rs 第127-138行，含使用场景说明）
- [x] 5. 运行 `cargo check` 验证编译通过
- [x] 6. 运行 `cargo test` 确保不影响现有功能

## 验证标准
- [ ] `cargo check` 通过
- [ ] 现有测试全部通过
- [ ] spawn_agent 工具能编译并派生子 agent
- [ ] 子 agent 能独立完成指定任务并输出结果

# 验证上下文压缩能力

## 目标
验证四层渐进式上下文压缩（层0工具修剪 → 层1滑动窗口 → 层2异步摘要 → 层3保底截断）是否真正工作，包括：
1. 各层级独立工作
2. 自动模式下的渐进触发
3. Token 缓存增量更新准确性
4. 异步摘要正确注入并删除原文
5. 动态 max_turns 按 Token 比例缩减
6. preserved 消息保护机制
7. 全链路端到端测试

## 执行步骤

- [x] 1. 分析现有测试覆盖，找出缺口（发现6个缺口，见分析）
- [x] 2. 编写增量测试：验证 Token 缓存增量更新的准确性（缓存 vs 全量重算一致性）
- [x] 3. 编写增量测试：验证 token-based 动态有效 max_turns 的触发逻辑
- [x] 4. 编写增量测试：验证异步摘要注入删除原文后 token 真实下降
- [x] 5. 编写增量测试：验证 end-to-end ContextManager 生命周期（多次 add_message → 压缩 → poll_summary）
- [x] 6. 运行所有测试验证通过（81 passed，6个新测试全部通过）
- [x] 7. 使用 spawn_agent 派生子 agent 验证压缩（子 agent 编译成功并运行了全部 81 个测试，全部通过）

## 验证标准
- [ ] 新测试全部通过（增量测试不破坏现有75个测试）
- [ ] Token 缓存增量更新与全量重算一致
- [ ] 动态 max_turns 在 token 超阈值时正确缩减
- [ ] 异步摘要注入后 token 总数正确下降
- [ ] 端到端场景下压缩按 0→1→2→3 渐进触发

# ✅ 验证 auto_compress 在真实 Agent 循环中工作

## 目标
验证四层渐进压缩在模拟真实 Agent 循环的场景中能正常工作，不仅仅是单元测试通过，而是：
1. 在持续 add_message（用户/助手/工具调用/工具结果）的循环中，压缩自动触发
2. 压缩后的上下文消息结构完整（系统提示词保留、消息顺序正确、类型正确）
3. Token 得到有效控制，不会无限增长
4. 压缩后的上下文仍能正常转换为 ChatMessage 列表供 LLM 使用

## 执行步骤
- [x] 1. 编写模拟真实 Agent 循环的集成测试（test_agent_loop_simulation_with_compression）
- [x] 2. 运行测试验证压缩在循环中实际触发（12轮中压缩被触发，token 受控在 600 以内）
- [x] 3. 运行全部已有测试，确保不破坏现有功能（82 passed，0 failed）
- [x] 4. 总结验证结果并更新 MEMORY.md

## 验证标准
- [x] 模拟 Agent 循环中压缩至少触发一次（compression_triggered = true）
- [x] 压缩后 token 数明显下降（≤600 < token_limit(300) × 2）
- [x] 系统提示词始终保留（system_count == 1）
- [x] 消息顺序和类型保持正确（第一条为 System，能正常转为 ChatMessage）
- [x] 全部 82 个已有测试仍然通过（82 passed）

# ✅ 修复 auto_compress 在实战中永不触发的 Bug

## 目标
修复上下文自动压缩在真实 Agent 循环中永不触发的 Bug。

## 根因分析
核心问题：
1. `effective_max_turns` 计算公式过于保守，token 超过阈值后 barely 降低
2. `hard_truncate` 使用 `>` 而非 `>=`，恰好 100% 时不会触发
3. 三层都跳过时返回 `NotNeeded`，表现为「压缩永不生效」

## 执行步骤

- [x] 1. **修复 `effective_max_turns` 计算公式**：改为线性缩放 `max_turns→1` 从 `trigger_threshold→token_limit`（strategy.rs:417-429）
- [x] 2. **修复 `hard_truncate` 触发条件**：`>` 改为 `>=`，确保 100% 时也能触发（strategy.rs:458）
- [x] 3. **增加诊断日志**：在 auto_compress 中加入 stderr 日志，输出每次检查的状态（strategy.rs:431-435, 463-468）
- [x] 4. **运行 `cargo check` 验证编译** — 通过
- [x] 5. **运行 `cargo test` 验证所有测试通过** — 82 passed
- [x] 6. **使用 test_agent_loop_simulation_with_compression 验证真实场景** — 触发压缩，token 受控

## 验证标准
- [x] `cargo check` 通过
- [x] 所有测试通过（82 passed）
- [x] 在模拟高频场景中压缩能正确触发
# 修复：触达限制后无行动（致命 bug）

## 目标
当前 `add_message()` → `check_and_compress()` → `auto_compress()` 四层压缩后，如果 Token 仍然超限（例如所有消息都是 protected），没有任何阻止机制，直接返回 `NotNeeded`。需要添加**紧急保底截断**机制。

## 修复步骤

- [x] 步骤1：分析现有代码，确认 `auto_compress` 在所有压缩层后仍超限的路径
- [x] 步骤2：新增 `emergency_truncate()` 函数（强制截断，仅保留 System + 最后 2 轮）
- [x] 步骤3：在 `auto_compress()` 中 hard_truncate 失败后调用 emergency_truncate
- [x] 步骤4：添加新的 `CompressResult::EmergencyTruncated` 变体
- [x] 步骤5：验证：`cargo check` 编译通过
- [x] 步骤6：运行单元测试确保不破坏现有逻辑（82 passed）

## 验证标准
1. ✅ `cargo check` 通过
2. ✅ `cargo test` 通过（82 passed）
3. ✅ 所有消息皆为 protected 时，emergency_truncate 能强制截断
# ✅ 启用 LLM 摘要（修复 setup_summary_channel）

## 目标
修复 `main.rs:167` 中 `ctx.setup_summary_channel(None)` 传入了 `None` 的问题，使得异步摘要器能使用 LLM 生成高质量的结构化摘要，而非退化为规则摘要。

## 执行步骤
- [x] 步骤1: 给 `OpenAiCompatibleAdapter` 添加 `Clone` derive（已有 `#[derive(Clone)]`）
- [x] 步骤2: 给 `ModelAdapter` trait 添加 `clone_box(&self) -> Box<dyn ModelAdapter>` 方法 + `Clone for Box<dyn ModelAdapter>`
- [x] 步骤3: 修改 `main.rs:167` 为 `Some(query_client.clone())`
- [x] 步骤4: 运行 `cargo check` 和 `cargo test` 验证

## 验证标准
1. ✅ `cargo check` 编译通过
2. ✅ `cargo test` 全部通过（90 passed）
3. ✅ LLM 摘要实际启用（异步摘要器能调用 LLM，LLM 不可用时自动降级为规则摘要）

# ✅ 会话管理功能

## 目标
为 Agent Lab 添加会话管理能力，支持保存、加载、列出、删除会话。

## 执行步骤

- [x] 1. 创建 `src/session/mod.rs` — 定义 SessionData、SessionManager（613行）
- [x] 2. 修改 `src/main.rs` — 注册 session 模块，添加 `/session` CLI 命令
- [x] 3. 编译验证 `cargo check`
- [x] 4. 用 spawn_agent 验证会话功能

## 验证标准
- [x] `cargo check` 通过
- [x] 支持 `/session save <name>` 保存当前对话
- [x] 支持 `/session list` 列出所有会话
- [x] 支持 `/session load <name>` 恢复对话
- [x] 支持 `/session delete <name>` 删除会话
- [x] 支持 `/session rename <old> <new>` 重命名会话

# 新增 `/` 命令系统（命令发现 + 帮助）

## 目标
增强 CLI 交互体验，让用户输入 `/` 时能弹出可用命令列表，帮助了解所有支持的命令。

## 设计思路
1. 创建 `src/commands/mod.rs` — 命令注册表（CommandRegistry）
2. 定义命令元数据：名称、描述、用法、子命令列表
3. 在 main.rs 中集成：
   - 当输入仅为 `/` 时，显示所有可用命令
   - 当输入 `/help` 时，显示详细帮助
   - 当输入未知 `/<xxx>` 时，提示未知命令 + 显示可用命令列表
   - 现有 `/session ...` 和 `/clear` 逻辑保留

## 执行步骤
- [x] 1. 创建 `src/commands/mod.rs` — CommandRegistry + Command 结构体，定义所有命令元数据
- [x] 2. 在 `main.rs` 中注册 commands 模块，集成到输入处理流程
- [x] 3. 实现 `/help` 命令显示所有可用命令
- [x] 4. 实现输入 `/` 时自动弹出命令列表
- [x] 5. 实现未知 `/` 命令的友好提示 + 可用命令列表
- [x] 6. 运行 `cargo check` 验证编译通过
- [x] 7. 运行 `cargo test` 确保不影响现有功能

## 验证标准
- [x] `cargo check` 通过
- [x] 现有 85 个测试全部通过（实际 90 passed）
- [x] 输入 `/` 显示可用命令列表
- [x] 输入 `/help` 显示详细帮助
- [x] 输入未知命令如 `/xyz` 提示友好信息

# 下一阶段 Roadmap：工具生态与自我进化

## 🎯 总体目标
补齐 Agent Lab 工具生态，构建动态工具发现机制，让 Agent 能自我认知并扩展能力。

---

## Phase 1: 动态工具系统与自我认知

### 目标
当前工具列表在系统提示词中硬编码，添加新工具后需要手动更新提示词。改为从 ToolManager 动态生成，让 Agent 永远知道自己有哪些工具可用。

### 执行步骤

- [ ] 1. 给 ToolManager 添加 `list_tools()` 方法，返回所有已注册工具的摘要（名称 + 描述）
- [ ] 2. 修改 `main.rs`，系统提示词中的「当前可用工具」部分改为从 ToolManager 动态生成
- [ ] 3. 创建 `/tools` CLI 命令，交互式列出所有可用工具及其参数 schema
- [ ] 4. 在 `commands/mod.rs` 中注册 `/tools` 命令
- [ ] 5. 验证：`cargo check` + `cargo test` 通过

### 验证标准
- [ ] `cargo check` 通过
- [ ] 所有现有测试通过（≥90 passed）
- [ ] 启动时系统提示词包含所有已注册工具的动态列表
- [ ] 输入 `/tools` 可列出所有工具及描述
- [ ] 添加新工具后自动出现在提示词和 `/tools` 输出中

---

## Phase 2: 网络能力扩展

### 目标
为 Agent 添加网络访问能力：HTTP 请求、文件下载。

### 执行步骤

- [ ] 1. 创建 `src/tools/http_tool/mod.rs` — HTTP 请求工具（GET/POST，支持 headers ）
- [ ] 2. 在 `main.rs` 中注册 http_tool
- [ ] 3. 创建 `src/tools/fetch_tool/mod.rs` — 文件下载工具（保存到本地）
- [ ] 4. 在 `main.rs` 中注册 fetch_tool
- [ ] 5. 验证：`cargo check` + `cargo test` 通过

### 验证标准
- [ ] `cargo check` 通过
- [ ] 所有测试通过
- [ ] HTTP 工具可发送 GET/POST 请求并返回响应
- [ ] 下载工具可将远程文件保存到本地

---

## Phase 3: MCP（Model Context Protocol）支持

### 目标
支持 MCP 协议，让 Agent 能连接 MCP 服务器动态加载工具，接入 MCP 生态。

### 执行步骤

- [ ] 1. 研究 MCP 协议规范（JSON-RPC based），设计客户端架构
- [ ] 2. 创建 `src/tools/mcp/mod.rs` — MCP 客户端实现
- [ ] 3. 实现工具发现（`tools/list`）和工具调用（`tools/call`）
- [ ] 4. 实现 MCP server 进程管理（启动/停止）
- [ ] 5. 在 `main.rs` 中集成 MCP 工具加载
- [ ] 6. 添加 MCP server 配置文件支持
- [ ] 7. 验证：`cargo check` + `cargo test` 通过

### 验证标准
- [ ] `cargo check` 通过
- [ ] 所有测试通过
- [ ] 能连接 MCP server 并获取工具列表
- [ ] 能调用 MCP server 提供的工具
- [ ] MCP server 异常断开时优雅降级

---

## Phase 4: Web UI 控制台

### 目标
提供 Web 界面替代纯 CLI 交互，方便可视化操作和多人协作。

### 执行步骤（初步调研）

- [ ] 1. 评估技术方案（SSE streaming vs WebSocket）
- [ ] 2. 设计架构（嵌入式 HTTP server + Web UI）
- [ ] 3. 实现基础原型

---

## 关键决策记录
（将记录在 MEMORY.md 中）
# Bug: 记忆压缩后导致后续调用结束

## 问题分析
记忆压缩（特别是 `hard_truncate`）可能破坏对话结构，导致后续 API 调用失败，
而 `ModelEvent::Error` 被静默忽略，使得 agent 提前退出。

### 根本原因
1. **`hard_truncate` 破坏对话结构**：当删除 Assistant(tool_calls) 消息但保留后续 Tool 消息时，
   对话结构变得无效（Tool 消息没有对应的 Assistant tool_calls 前驱），导致 API 调用失败。
2. **`ModelEvent::Error` 被静默忽略**：main.rs 中 `_ => ()` 忽略了 API 错误，
   导致 agent 认为没有 tool calls，从而退出 `--task` 模式。

## 修复步骤
- [x] 分析问题并编写 PLAN
- [x] 修复 `hard_truncate`：删除孤立 Tool 消息（无对应 Assistant tool_calls 前驱）
- [x] 处理 `ModelEvent::Error`：记录错误到 stderr，让 agent 知悉
- [x] 运行 `cargo check` 验证编译（通过）
- [x] 运行 `cargo test` 验证回归（90 passed）
# 实现全局 Debug 能力

## 目标
提供一个全局 debug 开关，当开启时，代码中所有 debug 判断的代码都能执行到（输出 debug 日志、启用调试行为等）。

## 执行步骤

- [x] **步骤 1：创建 debug 模块** — 创建 `src/debug/mod.rs`，提供全局 `AtomicBool` debug 标志 + 切换函数 + 检查宏
- [x] **步骤 2：注册 debug 模块** — 在 `main.rs` 中添加 `mod debug;` 声明
- [x] **步骤 3：添加 /debug CLI 命令** — 在 `main.rs` 的命令处理循环中支持 `/debug on|off|status`
- [x] **步骤 4：添加 DebugTool** — 实现一个工具，让 Agent 可以在运行时读取/设置 debug 标志
- [x] **步骤 5：注册 DebugTool** — 在 `initial_tool_manager()` 中注册 DebugTool
- [x] **步骤 6：注入到系统提示** — 在系统提示词中动态生成，包含 debug 工具描述
- [x] **步骤 7：编译验证** — 运行 `cargo check` 验证编译通过

## 验证标准
- `/debug on` 开启 debug 模式
- `/debug off` 关闭 debug 模式
- `/debug status` 显示当前状态
- Agent 可以通过工具调用 `debug` 工具来读取/设置 debug 标志
- `cargo check` 通过

# 修复：压缩后产生孤立的 Tool 消息导致 API 报错

## 目标
修复上下文压缩后出现 "Messages with role 'tool' must be a response to a preceding message with 'tool_calls'" 错误。

## 根因
压缩函数（sliding_window、emergency_truncate、force_compress、inject_summary）在移除消息时，可能删除 `Assistant(tool_calls)` 但保留对应的 `Tool` 响应消息（因 Tool 被标记为 preserved/important 或在保留范围内），导致向 API 发送的消息序列中产生孤立 Tool 消息，违反 OpenAI 协议。

## 执行步骤

- [ ] 0. 分析所有压缩路径，确认哪些函数需要修复
- [ ] 1. 提取 `hard_truncate` 中的孤儿 Tool 清理逻辑为独立函数 `remove_orphaned_tool_messages`
- [ ] 2. 在 `sliding_window_compress` 末尾调用孤儿清理
- [ ] 3. 在 `emergency_truncate` 末尾调用孤儿清理
- [ ] 4. 在 `force_compress` 的滑动窗口和保留消息路径末尾调用孤儿清理
- [ ] 5. 在 `inject_summary` 末尾调用孤儿清理
- [ ] 6. 在 `get_messages()` 中添加安全兜底（保险丝）
- [ ] 7. 运行 `cargo check 2>&1 | tail -30` 验证编译通过
- [ ] 8. 运行 `cargo test` 确保测试通过

## 验证标准
- [ ] `cargo check` 通过
- [ ] 所有现有测试通过
- [ ] 不再出现孤立 Tool 消息

# 修复压缩后孤立 Tool 消息

## 目标
确保所有压缩路径在删除消息后都清理孤立的 Tool 消息（Tool 消息对应的 Assistant tool_calls 已被删除），防止 OpenAI API 报错。

## 问题分析
`remove_orphaned_tool_messages()` 函数已在 3 个压缩路径中调用：
- ✅ `sliding_window_compress`（strategy.rs:274）
- ✅ `hard_truncate`（strategy.rs:455）
- ✅ `emergency_truncate`（strategy.rs:372）

但以下 2 个路径缺失：
- ❌ `force_compress` auto-loop 模式（手动构建消息后无清理）
- ❌ `inject_summary`（按 count 删除消息后可能产生孤儿）

## 执行步骤
- [x] 1. 在 `force_compress` auto-loop 路径中，手动构建消息列表后添加 `remove_orphaned_tool_messages` 调用
- [x] 2. 在 `inject_summary` 中，删除消息后添加 `remove_orphaned_tool_messages` 调用
- [x] 3. 运行 `cargo check` 验证编译通过
- [x] 4. 运行 `cargo test` 验证所有测试通过（94 passed）
- [x] 5. 修复测试 `test_sliding_window_with_tools_preserves_mid_turn`（孤儿 Tool 即使 preserved 也应清理，安全优先）
