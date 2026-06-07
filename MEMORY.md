

## [2025-01-xx] InvestigateTool 集成：自动错误快照 + 工具注册

### 关键决策
1. **注册 InvestigateTool**: 在 `default_tool_manager()` 中添加了 `InvestigateTool`，使其成为默认可用工具。
2. **自动捕获错误快照**: 在 Agent 主循环中，每次工具执行完成后，自动检测 `ok: false` 的工具结果，并调用 `ErrorSnapshotManager::capture()` 保存快照。
3. **快照输出**: 错误快照保存时，在 stderr 输出 `📸 错误快照已保存: <id> -> <path>`。
4. **[SNAPSHOT] 引用**: 在 `--task` 模式（子 agent）退出时，输出 `[SNAPSHOT] <id>` 引用最后的快照，方便主 agent 读取。

### 实现细节
- 快照捕获放在 `tool_results` 收集完成后、`tool_calls` 被移入 ChatMessage 之前
- 自动捕获只针对 `ok == false` 的工具结果
- 捕获内容包括：当前上下文消息、工具名称、工具参数、错误消息

## [2025-01-xx] CLI UX 优化第一阶段：output.rs 模块 + agent.rs 集成

### 关键决策
1. **创建 output.rs 专用模块**: 所有 CLI 输出样式集中在 `src/cli/output.rs`，包含：
   - `style` 子模块（ANSI 颜色 + 文本样式常量）
   - 状态徽章函数（badge_success/error/warning/info）
   - 分隔线与分区标题（separator/section/dim_section）
   - 表格渲染（table_header/table_row/kv_row/tag_value）
   - 加载动画（spinner_frame/loading_line/waiting_text/done_text）
   - 提示符生成（prompt/context_hint）
   - 启动横幅（welcome_banner）
   - 进度条（progress_bar）
   - 工具函数（truncate_str/format_duration/Timer）
2. **agent.rs 集成**: 将 output 模块集成到主循环的 5 个关键位置：
   - 启动时显示欢迎横幅
   - 用户提示符改为带颜色和上下文信息的样式
   - 工具调用可视化使用 section() 函数
   - 工具结果渲染使用 badge 和颜色常量
   - 等待动画使用 waiting_text 函数
3. **format! Unicode 问题**: `format!("{}{}━{}{}{}━{}{}", ...)` 格式字符串在 Rust 中解析异常，改用 `String::push_str()` 拼接解决。

### 当前状态
- 编译通过 (`cargo build` 成功)
- 后续步骤 6-10 待继续（会话管理表格化、流式输出优化、警告/错误视觉层次等）

# 最终修复：清理 agent.rs 中的原始 ANSI 转义码

**时间**: 2025-07-17

## 问题
`agent.rs` 中有多处硬编码的 ANSI 转义码（如 `\x1b[32m`、`\x1b[90m`、`\x1b[31m` 等），以及 `"名称" + output::style::FG_CYAN + "..."` 的字符串拼接 bug（String + &str 编译错误）。

## 修改内容

### 1. `src/agent.rs` — 修复字符串拼接 + 替换 ANSI 码
- `/tools` 命令：修复 `"名称" + output::style::FG_CYAN + output::style::RESET` → `format!(...output::style::FG_CYAN...)`
- `/debug` 命令：替换所有 `println!("\x1b[32m...\x1b[0m")` → `println!("{}", output::badge_success("..."))`
  - 以及 `\x1b[33m` → `badge_warning()`, `\x1b[36m` → `output::section()`, `\x1b[90m` → `output::style::FG_BRIGHT_BLACK`
- `ModelEvent::Thinking` 输出：替换 `\x1b[90m...\x1b[0m` → `output::thinking_text(&content)`
- `ModelEvent::Error` 输出：替换 `\x1b[31m❌ 模型 API 错误: {}\x1b[0m` → 使用 `output::style::FG_RED`
- `/session` 处理函数：替换所有原始 ANSI 码为 `output::` 函数调用
- `list_sessions` 修复：`session.messages.len()` → `session.message_count`（SessionInfo 字段）

### 2. `src/cli/output.rs` — 更新函数签名 + 添加新函数
- `badge_success()` → `badge_success(msg: &str)` — 新增 msg 参数，输出: `{FG_GREEN}{BOLD} ✅ {msg}{RESET}`
- `badge_warning()` → `badge_warning(msg: &str)` — 新增 msg 参数，输出: `{FG_YELLOW}{BOLD} ⚠️ {msg}{RESET}`
- 新增 `thinking_text(content: &str)` — 输出: `{FG_BRIGHT_BLACK}{content}{RESET}`

## 验证
- `cargo check` 通过 ✅（0 errors, 仅有预存的 warnings）
