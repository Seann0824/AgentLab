# 功能特性设计：上下文窗口管理 (Context Window Management)

> **优先级**: P0 - 最高优先级  
> **状态**: 待实现  
> **估算工时**: 1-2 天  
> **关联**: 当前 `messages: Vec<ChatMessage>` 无限制增长

---

## 1. 问题陈述

### 1.1 当前架构的问题

当前 `main.rs` 的主循环中，消息以最简单的方式累积：

```rust
let mut messages: Vec<ChatMessage> = vec![
    ChatMessage::system("..."),  // 系统提示词
];

loop {
    // 用户输入 → push
    messages.push(ChatMessage::user(input));

    // 模型回复 → push
    messages.push(ChatMessage::assistant(response));

    // 工具调用 → push
    messages.push(ChatMessage::assistant_tool_calls(...));
    messages.push(ChatMessage::tool(result));

    // 永不删除，永不压缩
}
```

### 1.2 具体影响分析

| 问题 | 触发条件 | 后果 | 严重程度 |
|------|---------|------|---------|
| **Token 超限** | 对话 10-15 轮后 | API 返回 400 `context_length_exceeded` | 🔴 系统崩溃 |
| **成本浪费** | 每轮对话都发全部历史 | DeepSeek 定价：每百万 Token $0.5-2，冗余 50%+ | 🟡 持续损耗 |
| **推理退化** | 上下文 > 32K tokens | 模型"迷失"在大量工具输出中，忽略最新指令 | 🟠 质量下降 |
| **延迟增加** | 上下文 > 16K tokens | API 响应时间与输入长度正相关 | 🟡 体验下降 |

### 1.3 实测数据

以一个典型的"修改代码"场景为例：

| 轮次 | 动作 | 消息数 | 估算 Token | 状态 |
|------|------|--------|-----------|------|
| 1 | 用户: 读取 main.rs | 2 | ~200 | ✅ |
| 2 | 模型 cat main.rs (100行) | 4 | ~3,000 | ✅ |
| 3 | 模型 修改代码 | 6 | ~3,200 | ✅ |
| 4 | 模型 cargo build (输出200行) | 8 | ~7,000 | ✅ |
| 5 | 用户: 再改个文件 | 10 | ~7,500 | ✅ |
| ... | ... | ... | ... | ... |
| 10 | 第10轮 | 20 | ~25,000 | ⚠️ 接近限制 |
| 15 | 第15轮 | 30 | ~50,000+ | 🔴 超限崩溃 |

---

## 2. 功能需求

### 2.1 核心需求

| ID | 需求描述 | 验收标准 |
|----|---------|---------|
| C-01 | **滑动窗口**：只保留最近 N 轮对话，自动丢弃早期历史 | 配置为 5 轮时，messages 中最多保留 5 轮用户+助手交互 |
| C-02 | **Token 计数**：估算当前上下文的 Token 消耗 | 每轮对话后显示 `[Token: ~12,345 / 128K]` |
| C-03 | **自动压缩**：接近 Token 限制时自动触发摘要压缩 | 达到 70% 限制时，自动将早期对话压缩为一段摘要 |
| C-04 | **系统提示词保护**：系统提示词始终保留，不被压缩丢弃 | 即使窗口滑动，`ChatMessage::System` 始终在第一位 |
| C-05 | **可配置策略**：用户可通过配置选择压缩策略 | 支持 `sliding_window`、`summarization`、`auto` 三种模式 |

### 2.2 非功能性需求

| 需求 | 指标 |
|------|------|
| Token 估算精度 | ±20% 以内（无需精确计数，够用即可） |
| 压缩触发时机 | 每次模型返回后执行检查 |
| 摘要压缩延迟 | < 500ms（调用轻量模型或本地规则） |
| 配置热加载 | 支持运行时切换策略（可选）

---

## 3. 技术方案

### 3.1 架构位置

```
src/
├── main.rs                    # 集成 ContextManager
└── context/                   # 新增：上下文管理模块
    ├── mod.rs                 # ContextManager 核心 + 公开 API
    ├── tokenizer.rs           # Token 估算器（轻量级估算，无需依赖分词库）
    ├── strategy.rs            # 压缩策略：滑动窗口、摘要压缩、自动
    └── config.rs              # 配置定义与加载
```

### 3.2 核心数据结构

#### 3.2.1 策略配置

```rust
/// 上下文压缩策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextStrategy {
    /// 滑动窗口：只保留最近 N 轮对话
    SlidingWindow {
        /// 保留的最大轮数（用户+助手算一轮）
        max_turns: usize,
    },
    /// 摘要压缩：将早期对话压缩为一段摘要消息
    Summarization {
        /// 触发压缩的 Token 阈值（占限制的百分比 0.0~1.0）
        trigger_ratio: f64,
        /// 压缩后保留的最近轮数（摘要前面 + 最近 N 轮）
        keep_recent_turns: usize,
    },
    /// 自动模式：先滑动窗口，接近限制时触发摘要
    Auto {
        /// Token 硬限制（模型上下文窗口大小）
        token_limit: usize,
        /// 滑动窗口保留轮数
        max_turns: usize,
        /// 摘要触发比例
        trigger_ratio: f64,
    },
}

impl Default for ContextStrategy {
    fn default() -> Self {
        // DeepSeek V4 上下文窗口按 128K 估算
        ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,  // 70% → ~89K tokens 时触发
        }
    }
}
```

#### 3.2.2 上下文管理器

```rust
/// 上下文管理器
///
/// 职责：
/// 1. 管理消息列表的生命周期
/// 2. 估算 Token 消耗
/// 3. 根据策略自动压缩上下文
/// 4. 保护系统提示词不被删除
pub struct ContextManager {
    /// 完整消息列表（包含已压缩的历史摘要）
    messages: Vec<ChatMessage>,
    /// 压缩策略
    strategy: ContextStrategy,
    /// Token 估算器
    tokenizer: TokenEstimator,
    /// 统计信息
    stats: ContextStats,
}

/// 上下文统计信息
#[derive(Debug, Clone)]
pub struct ContextStats {
    /// 当前估算 Token 数
    pub estimated_tokens: usize,
    /// 消息总数
    pub message_count: usize,
    /// 对话轮数
    pub turn_count: usize,
    /// 是否已触发压缩
    pub compressed: bool,
    /// 最后压缩时间
    pub last_compressed_at: Option<std::time::Instant>,
    /// Token 使用率 (%)
    pub usage_ratio: f64,
}

impl ContextManager {
    /// 创建新的上下文管理器
    pub fn new(system_prompt: String, strategy: ContextStrategy) -> Self { ... }

    /// 添加消息（自动触发压缩检查）
    pub fn add_message(&mut self, message: ChatMessage) { ... }

    /// 获取当前消息列表（发送给模型）
    pub fn get_messages(&self) -> &[ChatMessage] { ... }

    /// 手动触发压缩
    pub fn compress(&mut self) -> CompressResult { ... }

    /// 获取统计信息
    pub fn stats(&self) -> &ContextStats { ... }

    /// 切换压缩策略
    pub fn set_strategy(&mut self, strategy: ContextStrategy) { ... }
}
```

### 3.3 Token 估算器 (`tokenizer.rs`)

不需要引入 `tiktoken-rs` 等外部依赖，采用**经验公式估算**：

```rust
/// 轻量级 Token 估算器
///
/// 估算规则：
/// - 英文: ~4 chars/token
/// - 中文: ~1.5 chars/token
/// - 代码: ~3 chars/token
/// - 特殊字符（空格、换行）: ~0.5 token/char
///
/// 精度目标: ±20%
pub struct TokenEstimator;

impl TokenEstimator {
    /// 估算文本的 Token 数
    pub fn estimate_text(text: &str) -> usize {
        let mut tokens = 0f64;
        let mut ascii_count = 0usize;
        let mut cjk_count = 0usize;

        for ch in text.chars() {
            match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' => ascii_count += 1,
                '\u{4e00}'..='\u{9fff}' | '\u{3000}'..='\u{303f}' => cjk_count += 1,
                ' ' | '\n' | '\t' => tokens += 0.25,  // 空白符很省 token
                _ => tokens += 0.5,  // 标点、符号等
            }
        }

        tokens += ascii_count as f64 / 4.0;   // 英文约 4 字符/token
        tokens += cjk_count as f64 / 1.5;     // 中文约 1.5 字符/token

        // 向上取整，+1 保底
        (tokens.ceil() as usize).max(1)
    }

    /// 估算 ChatMessage 的 Token 数
    pub fn estimate_message(msg: &ChatMessage) -> usize {
        let role_tokens = match msg {
            ChatMessage::System { content } => 4 + Self::estimate_text(content),
            ChatMessage::User { content } => 3 + Self::estimate_text(content),
            ChatMessage::Assistant { content, tool_calls } => {
                let mut t = 3 + Self::estimate_text(content);
                for tc in tool_calls {
                    t += 8;  // tool_call 结构开销
                    t += Self::estimate_text(&tc.name);
                    t += Self::estimate_text(&tc.arguments);
                }
                t
            }
            ChatMessage::Tool { tool_call_id, content } => {
                3 + Self::estimate_text(tool_call_id) + Self::estimate_text(content)
            }
        };
        role_tokens
    }

    /// 估算消息列表的总 Token 数
    pub fn estimate_messages(messages: &[ChatMessage]) -> usize {
        messages.iter()
            .map(|msg| Self::estimate_message(msg))
            .sum::<usize>()
            + 10  // 消息列表包装开销
    }

    /// 格式化 Token 数（人类可读）
    pub fn format_tokens(count: usize) -> String {
        if count > 1_000_000 {
            format!("{:.1}M", count as f64 / 1_000_000.0)
        } else if count > 1_000 {
            format!("{:.1}K", count as f64 / 1_000.0)
        } else {
            format!("{}", count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_english() {
        let tokens = TokenEstimator::estimate_text("Hello, world! This is a test.");
        assert!(tokens > 0);
        assert!(tokens < 20);  // ~8 tokens
    }

    #[test]
    fn test_estimate_chinese() {
        let tokens = TokenEstimator::estimate_text("你好世界，这是一个测试。");
        assert!(tokens > 0);
        assert!(tokens < 20);  // ~8 tokens
    }

    #[test]
    fn test_estimate_code() {
        let code = r#"fn main() { println!("hello"); }"#;
        let tokens = TokenEstimator::estimate_text(code);
        assert!(tokens > 0);
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(TokenEstimator::format_tokens(500), "500");
        assert!(TokenEstimator::format_tokens(1500).contains("K"));
        assert!(TokenEstimator::format_tokens(1_500_000).contains("M"));
    }
}
```

### 3.4 压缩策略实现 (`strategy.rs`)

#### 3.4.1 滑动窗口策略

```rust
impl ContextStrategy {
    /// 执行上下文压缩
    pub fn compress(
        &self,
        messages: &mut Vec<ChatMessage>,
        stats: &mut ContextStats,
    ) -> CompressResult {
        match self {
            ContextStrategy::SlidingWindow { max_turns } => {
                Self::sliding_window_compress(messages, *max_turns, stats)
            }
            ContextStrategy::Summarization { trigger_ratio, keep_recent_turns } => {
                Self::summarization_compress(messages, *trigger_ratio, *keep_recent_turns, stats)
            }
            ContextStrategy::Auto { token_limit, max_turns, trigger_ratio } => {
                Self::auto_compress(messages, *token_limit, *max_turns, *trigger_ratio, stats)
            }
        }
    }

    /// 滑动窗口压缩
    ///
    /// 保留规则：
    /// 1. 系统提示词始终保留
    /// 2. 保留最近 N 轮对话
    /// 3. 工具调用的中间消息（assistant_tool_calls + tool）视为一轮
    fn sliding_window_compress(
        messages: &mut Vec<ChatMessage>,
        max_turns: usize,
        stats: &mut ContextStats,
    ) -> CompressResult {
        let original_count = messages.len();

        // 找到系统提示词（必须保留）
        let system_msgs: Vec<ChatMessage> = messages
            .iter()
            .filter(|m| matches!(m, ChatMessage::System { .. }))
            .cloned()
            .collect();

        // 计算轮数
        let turns = Self::count_turns(messages);

        if turns <= max_turns {
            return CompressResult::NotNeeded;
        }

        // 需要移除的轮数
        let remove_turns = turns - max_turns;

        // 找到要保留的起始位置（跳过系统提示词）
        let mut keep_start = 0;
        for (i, msg) in messages.iter().enumerate() {
            if !matches!(msg, ChatMessage::System { .. }) {
                keep_start = i;
                break;
            }
        }

        // 计算需要删除的消息数量
        let mut turns_removed = 0;
        let mut delete_end = keep_start;

        for i in keep_start..messages.len() {
            if turns_removed >= remove_turns {
                delete_end = i;
                break;
            }
            // 检测一轮结束：Assistant 消息（无 tool_calls）或 Done
            if let ChatMessage::Assistant { tool_calls, .. } = &messages[i] {
                if tool_calls.is_empty() {
                    turns_removed += 1;
                }
            }
            // 一轮也可能由 Tool 消息后接 User 消息结束
            if i + 1 < messages.len() {
                if matches!(&messages[i], ChatMessage::Tool { .. })
                    && matches!(&messages[i + 1], ChatMessage::User { .. })
                {
                    turns_removed += 1;
                }
            }
            delete_end = i + 1;
        }

        if delete_end > keep_start {
            let removed = messages.drain(keep_start..delete_end);
            let removed_count = removed.len();

            stats.compressed = true;
            stats.last_compressed_at = Some(std::time::Instant::now());

            return CompressResult::Compressed {
                removed_count,
                removed_turns: remove_turns,
                remaining_count: messages.len(),
                summary: format!(
                    "已移除最早 {} 轮对话（{} 条消息），共节省约 {} tokens",
                    remove_turns,
                    removed_count,
                    TokenEstimator::estimate_messages(&[])
                ),
            };
        }

        CompressResult::NotNeeded
    }

    /// 统计对话轮数
    fn count_turns(messages: &[ChatMessage]) -> usize {
        let mut turns = 0;
        for msg in messages {
            match msg {
                ChatMessage::User { .. } => turns += 1,
                ChatMessage::Assistant { tool_calls, .. } => {
                    if tool_calls.is_empty() {
                        // 纯文本回复结束一轮
                    }
                }
                _ => {}
            }
        }
        turns
    }
}

/// 压缩结果
pub enum CompressResult {
    /// 无需压缩
    NotNeeded,
    /// 已压缩
    Compressed {
        removed_count: usize,
        removed_turns: usize,
        remaining_count: usize,
        summary: String,
    },
    /// 需要生成摘要（异步操作）
    NeedsSummarization {
        messages_to_summarize: Vec<ChatMessage>,
        keep_recent: Vec<ChatMessage>,
    },
}
```

#### 3.4.2 自动模式核心逻辑

```rust
/// 自动模式：滑动窗口 + 摘要压缩两阶段
fn auto_compress(
    messages: &mut Vec<ChatMessage>,
    token_limit: usize,
    max_turns: usize,
    trigger_ratio: f64,
    stats: &mut ContextStats,
) -> CompressResult {
    let current_tokens = TokenEstimator::estimate_messages(messages);
    let trigger_threshold = (token_limit as f64 * trigger_ratio) as usize;

    stats.estimated_tokens = current_tokens;
    stats.usage_ratio = current_tokens as f64 / token_limit as f64;

    if current_tokens < trigger_threshold {
        return CompressResult::NotNeeded;
    }

    // 阶段 1：先尝试滑动窗口（轻量，无需 LLM 调用）
    let turns = Self::count_turns(messages);
    if turns > max_turns {
        return Self::sliding_window_compress(messages, max_turns, stats);
    }

    // 阶段 2：滑动窗口不够，需要摘要压缩
    // 将早期消息标记为待摘要
    let system_msgs: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| matches!(m, ChatMessage::System { .. }))
        .cloned()
        .collect();

    // 找到要保留的最近消息
    let keep_count = std::cmp::min(max_turns, 10) * 2 + system_msgs.len();
    let start = if messages.len() > keep_count {
        messages.len() - keep_count
    } else {
        return CompressResult::NotNeeded;
    };

    let to_summarize: Vec<ChatMessage> = messages
        .drain(system_msgs.len()..start)
        .collect();

    if !to_summarize.is_empty() {
        stats.compressed = true;
        return CompressResult::NeedsSummarization {
            messages_to_summarize: to_summarize,
            keep_recent: messages.clone(),
        };
    }

    CompressResult::NotNeeded
}
```

### 3.5 系统集成（与 main.rs 的集成）

```rust
// 在 main.rs 中修改

use crate::context::ContextManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let query_client = initial_model()?;
    let tool_manager = initial_tool_manager();

    let policy_summary = /* 权限摘要 */;

    // 使用 ContextManager 替代 Vec<ChatMessage>
    let mut ctx = ContextManager::new(
        format!(
            "你当前工作的目录为 ...。\n\n{}",
            policy_summary
        ),
        ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,
        },
    );

    let mut is_auto = false;
    let mut terminal_line_dirty = false;

    loop {
        if !is_auto {
            let mut user_input = String::new();
            finish_terminal_line(&mut terminal_line_dirty);
            print!(">");
            std::io::Write::flush(&mut std::io::stdout())?;
            if std::io::stdin().read_line(&mut user_input).is_err() {
                continue;
            }
            if user_input.trim().is_empty() {
                continue;
            }
            ctx.add_message(ChatMessage::user(user_input));
        }

        // 显示当前的 Token 使用状态
        let stats = ctx.stats().clone();
        if stats.usage_ratio > 0.5 {
            eprint!(
                "\r\x1b[2K[Token: {}/{} ({:.0}%)] ",
                TokenEstimator::format_tokens(stats.estimated_tokens),
                TokenEstimator::format_tokens(128_000),
                stats.usage_ratio * 100.0,
            );
        }

        let mut stream_chat = query_client.stream_chat(
            ctx.get_messages(),  // 传入当前上下文
            tool_manager.get_tools_scehma(),
        );

        // ... 后续保持不变
    }
}
```

### 3.6 摘要压缩的实现（可选）

当自动模式触发 `NeedsSummarization` 时，需要将早期对话压缩为一段摘要。有两种实现方式：

#### 方案 A：调用自身模型做摘要（推荐）

```rust
/// 使用 LLM 生成对话摘要
async fn summarize_messages(
    model: &dyn ModelAdapter,
    messages: &[ChatMessage],
) -> anyhow::Result<String> {
    let summary_prompt = ChatMessage::user(format!(
        "请用中文总结以下对话的核心内容（目标、已执行的操作、关键发现）。\
         保留技术细节和文件路径。控制在 200 字以内。\n\n{}",
        format_messages_for_summary(messages)
    ));

    let mut stream = model.stream_chat(
        &[ChatMessage::system("你是一个精准的对话摘要助手。"), summary_prompt],
        serde_json::json!([]),
    );

    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let ModelEvent::Text(text) = event {
            summary.push_str(&text);
        }
    }

    Ok(summary)
}
```

#### 方案 B：规则摘要（轻量，无需 LLM 调用）

```rust
/// 基于规则的摘要生成（无需模型调用）
fn rule_based_summary(messages: &[ChatMessage]) -> String {
    let mut user_intents = Vec::new();
    let mut tool_actions = Vec::new();
    let mut key_files = Vec::new();

    for msg in messages {
        match msg {
            ChatMessage::User { content } => {
                // 提取用户意图前 50 字
                let intent: String = content.chars().take(50).collect();
                user_intents.push(intent);
            }
            ChatMessage::Tool { content, .. } => {
                // 提取命令执行摘要
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                    if let Some(result) = val.get("result") {
                        if let Some(cmd) = result.get("command") {
                            tool_actions.push(cmd.as_str().unwrap_or("").to_string());
                        }
                    }
                }
            }
            ChatMessage::Assistant { content, .. } => {
                // 提取引用的文件路径
                for word in content.split_whitespace() {
                    if word.contains('/') && (word.ends_with(".rs") || word.ends_with(".toml")) {
                        key_files.push(word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/' && c != '.'));
                    }
                }
            }
            _ => {}
        }
    }

    // 去重
    user_intents.dedup();
    tool_actions.dedup();
    key_files.dedup();

    let mut summary = String::from("【历史对话摘要】\n");
    if !user_intents.is_empty() {
        summary.push_str(&format!("用户意图: {}\n", user_intents.join("; ")));
    }
    if !tool_actions.is_empty() {
        summary.push_str(&format!("执行操作: {}\n", tool_actions.join(", ")));
    }
    if !key_files.is_empty() {
        summary.push_str(&format!("涉及文件: {}\n", key_files.join(", ")));
    }

    summary
}
```

---

## 4. 实现计划

```
Phase 1: Token 估算器（0.25天）
├── 实现 TokenEstimator（基于字符统计的经验公式）
├── 单元测试：英文、中文、代码、混合文本
└── 集成 test 验证估算精度

Phase 2: 压缩策略（0.5天）
├── 实现 ContextStrategy::SlidingWindow
├── 实现 ContextStrategy::Auto
├── 实现对话轮数计数器
└── 单元测试：窗口滑动边界条件、消息保留正确性

Phase 3: ContextManager 核心（0.25天）
├── 实现 ContextManager（封装消息列表 + 自动压缩触发）
├── 实现统计信息收集 (ContextStats)
├── 系统提示词保护机制
└── 单元测试：完整生命周期

Phase 4: 系统集成（0.25天）
├── 修改 main.rs 使用 ContextManager 替代 Vec<ChatMessage>
├── 添加 Token 使用率显示（终端状态栏）
├── 集成测试：长时间对话不崩溃
└── 文档

总计: 1.25 天
```

---

## 5. 测试策略

### 5.1 单元测试

| 测试用例 | 目标 |
|---------|------|
| `test_estimate_short_text` | 短文本 Token 估算 |
| `test_estimate_long_code` | 代码文本 Token 估算 |
| `test_estimate_message_types` | 各消息类型 Token 估算 |
| `test_sliding_window_basic` | 基本滑动窗口功能 |
| `test_sliding_window_protects_system` | 系统提示词不被删除 |
| `test_sliding_window_exact_limit` | 恰好等于窗口大小的边界 |
| `test_sliding_window_below_limit` | 低于窗口大小不触发 |
| `test_auto_no_compression_needed` | 低 Token 使用不触发 |
| `test_auto_triggers_at_threshold` | 超过阈值触发压缩 |
| `test_context_manager_add_and_compress` | 完整添加+压缩流程 |
| `test_context_manager_stats` | 统计信息正确性 |
| `test_rule_based_summary` | 规则摘要内容格式 |

### 5.2 集成测试

```rust
#[tokio::test]
async fn test_long_conversation_does_not_crash() {
    let mut ctx = ContextManager::new(
        "system prompt".to_string(),
        ContextStrategy::Auto {
            token_limit: 10_000,    // 小限制方便测试
            max_turns: 3,
            trigger_ratio: 0.5,
        },
    );

    // 模拟 50 轮对话
    for i in 0..50 {
        ctx.add_message(ChatMessage::user(format!("user message {}", i)));

        // 模拟工具调用
        ctx.add_message(ChatMessage::assistant_tool_calls(
            format!("thinking {}", i),
            vec![ToolCall {
                id: format!("call_{}", i),
                name: "shell".into(),
                arguments: r#"{"command": "echo ok"}"#.into(),
            }],
        ));

        ctx.add_message(ChatMessage::tool(
            format!("call_{}", i),
            r#"{"ok": true, "result": {"stdout": "ok\n"}}"#.into(),
        ));

        ctx.add_message(ChatMessage::assistant(format!("response {}", i)));

        // 验证系统提示词始终存在
        assert!(ctx.get_messages().iter().any(|m| matches!(m, ChatMessage::System { .. })));
    }

    // 验证压缩后的消息数不会无限增长
    assert!(ctx.get_messages().len() < 50, "消息数应被压缩控制");
    let stats = ctx.stats();
    assert!(stats.compressed, "应已触发压缩");
}
```

---

## 6. 与现有系统的集成

### 6.1 模块注册

在 `src/main.rs` 中添加模块声明：

```rust
mod context;
```

在 `Cargo.toml` 中无需新增依赖（估算器自实现，不依赖外部 Tokenizer）。

### 6.2 系统提示词补充

建议在系统提示词中追加以下内容，让模型理解上下文可能被压缩：

```
注意：为了管理上下文窗口，早期对话历史可能会被自动压缩为摘要。
如果发现某些上下文缺失，请基于摘要信息继续工作。
重要的上下文信息请保留在文件中，而不是仅依赖对话历史。
```

### 6.3 终端显示效果

```
> 帮我修改 src/main.rs
[Token: 23.5K/128K (18%)]
... 模型流式输出 ...

> cargo build
[Token: 45.2K/128K (35%)]
... 编译输出 ...

> (自动压缩: 已移除最早 3 轮对话，节省 ~12K tokens)
[Token: 38.1K/128K (30%)]
```

---

## 7. 风险评估

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|---------|
| Token 估算不精确导致过早/过晚压缩 | 中 | 高 | 经验公式 ±20% 足够；触发阈值留余量（70% 触发） |
| 摘要压缩丢失关键信息 | 高 | 中 | 规则摘要保留文件路径和命令；LLM 摘要可用更精准 |
| 滑动窗口切断了工具调用的完整上下文 | 中 | 低 | 按"轮"切割（User→Tool→Assistant 为一轮），不会切断中间消息 |
| 性能影响（每次添加消息都估算 Token） | 低 | 低 | 估算时间复杂度 O(n)，消息通常 < 100 条，< 1ms |

---

## 8. 后续扩展方向

| 阶段 | 功能 | 说明 |
|------|------|------|
| V1 | 滑动窗口 + Token 显示 | 本方案核心内容 |
| V2 | 基于 LLM 的摘要压缩 | 调用模型自身做智能摘要 |
| V3 | 持久化压缩索引 | 压缩的内容存入文件，模型可"翻页"查看 |
| V4 | 自适应窗口 | 根据当前对话复杂度动态调整窗口大小 |
| V5 | 结构化上下文 | 将文件内容、工具输出、对话分开展示给模型 |

---

## 9. 为什么这是 P0

1. **系统稳定性**：不做上下文管理，对话超过 10-15 轮必然崩溃，系统**不可用**
2. **成本控制**：每次 API 调用都发送全部历史，Token 消耗随对话轮次线性增长
3. **用户体验**：Token 消耗透明化（终端显示），用户可以感知上下文状态
4. **与权限沙箱互补**：权限沙箱让模型"安全地做事"，上下文管理让模型"持续地做事"

---

> **设计原则**: 上下文窗口管理应该做到"透明、自动、无损"。
> **核心哲学**: 不是限制对话长度，而是让有限窗口内保留最有价值的信息。
> 与 Skill 理念一致——让 AI 专注当前任务，不被历史淹没。

