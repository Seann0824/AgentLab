# 功能特性设计：上下文窗口管理 (Context Window Management)

> **优先级**: P0 - 最高优先级  
> **状态**: 待实现  
> **估算工时**: 1.5-2 天  
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
| C-03 | **自动降级**：接近 Token 限制时自动触发滑动窗口或摘要压缩 | 达到 70% 限制时，自动将早期对话压缩为一段摘要 |
| C-04 | **系统提示词保护**：系统提示词始终保留，不被压缩丢弃 | 即使窗口滑动，`ChatMessage::System` 始终在第一位 |
| C-05 | **重要上下文标记**：支持标记关键消息为"永久保留"，不被滑动窗口丢弃 | 标记为 `preserve: true` 的消息不会被压缩移除 |
| C-06 | **可配置策略**：用户可通过配置选择压缩策略 | 支持 `sliding_window`、`auto` 两种模式 |

### 2.2 非功能性需求

| 需求 | 指标 |
|------|------|
| Token 估算精度 | ±20% 以内（够用即可，需用 tiktoken 交叉验证校准） |
| 压缩触发时机 | 每次模型返回后执行检查 |
| 压缩操作延迟 | < 1ms（滑动窗口纯内存操作，不得阻塞事件循环） |
| 摘要生成 | 异步执行，不阻塞主循环 |
| 配置热加载 | 支持运行时切换策略（可选） |

---

## 3. 技术方案

### 3.1 架构位置

```
src/
├── main.rs                    # 集成 ContextManager
└── context/                   # 新增：上下文管理模块
    ├── mod.rs                 # ContextManager 核心 + 公开 API
    ├── tokenizer.rs           # Token 估算器（轻量级估算 + 定期校准接口）
    ├── strategy.rs            # 压缩策略：滑动窗口、自动模式
    ├── config.rs              # 配置定义与加载
    └── summarizer.rs          # 异步摘要生成器（后台任务）
```

### 3.2 核心数据结构

#### 3.2.1 策略配置

```rust
/// 上下文压缩策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextStrategy {
    /// 滑动窗口：只保留最近 N 轮对话
    /// 适合不需要长期记忆的简单任务场景
    SlidingWindow {
        /// 保留的最大轮数（用户+助手算一轮）
        max_turns: usize,
    },
    /// 自动模式：滑动窗口兜底 + 异步摘要压缩
    /// 适合需要长时间持续工作的 Agent 场景
    Auto {
        /// Token 硬限制（模型上下文窗口大小）
        token_limit: usize,
        /// 滑动窗口保留轮数（压缩后的保底）
        max_turns: usize,
        /// 触发压缩的 Token 阈值（占限制的百分比 0.0~1.0）
        trigger_ratio: f64,
        /// 是否启用异步摘要（默认启用）
        enable_async_summary: bool,
    },
}

impl Default for ContextStrategy {
    fn default() -> Self {
        // DeepSeek V4 上下文窗口按 128K 估算
        ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,  // 70% → ~89K tokens 时触发
            enable_async_summary: true,
        }
    }
}
```

#### 3.2.2 ⭐ 消息重要性标记

这是本次方案的核心改进之一：**不是所有消息都平等**。

```rust
/// 扩展的消息类型，增加持久化标记
#[derive(Debug, Clone)]
pub struct ContextMessage {
    /// 原始 ChatMessage
    pub message: ChatMessage,
    /// 是否标记为"永久保留"
    /// 标记后不会被滑动窗口丢弃，仅在所有策略都无效时由摘要处理
    pub preserved: bool,
    /// 消息的重要性标签（辅助摘要决策）
    pub importance: MessageImportance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageImportance {
    /// 普通对话消息
    Normal,
    /// 关键上下文（如文件读取结果、项目结构发现）
    Important,
    /// 里程碑决策（如选择了某个技术方案）
    Milestone,
}

impl ContextMessage {
    /// 标记为永久保留
    pub fn preserve(mut self) -> Self {
        self.preserved = true;
        self.importance = MessageImportance::Important;
        self
    }

    /// 自动判断重要性
    pub fn auto_classify(msg: &ChatMessage) -> MessageImportance {
        match msg {
            ChatMessage::Tool { content, .. } => {
                // 文件读取、目录列表等包含结构性信息的结果，标记为重要
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                    if let Some(result) = val.get("result") {
                        if let Some(stdout) = result.get("stdout").and_then(|s| s.as_str()) {
                            // 包含文件路径列表、代码内容等
                            if stdout.contains("─") || stdout.contains(".rs")
                                || stdout.contains("Cargo.toml")
                                || stdout.lines().count() > 5
                            {
                                return MessageImportance::Important;
                            }
                        }
                    }
                }
                MessageImportance::Normal
            }
            ChatMessage::Assistant { content, .. } => {
                // 包含重要决策标记的回复
                if content.contains("【决策】") || content.contains("方案选择") {
                    MessageImportance::Milestone
                } else {
                    MessageImportance::Normal
                }
            }
            _ => MessageImportance::Normal,
        }
    }
}

impl From<ChatMessage> for ContextMessage {
    fn from(message: ChatMessage) -> Self {
        let importance = ContextMessage::auto_classify(&message);
        ContextMessage {
            message,
            preserved: false,
            importance,
        }
    }
}
```

> **设计意图**：Agent 场景下，早期读取的**项目结构、文件内容、架构决策**是后续所有工作的基础。这些信息一旦被滑动窗口丢掉，模型会"失忆"——比如重新读取已经读过的文件。重要性标记让关键信息获得"豁免权"。

#### 3.2.3 上下文管理器

```rust
/// 上下文管理器
///
/// 职责：
/// 1. 管理消息列表的生命周期
/// 2. 估算 Token 消耗（增量式）
/// 3. 根据策略自动压缩上下文
/// 4. 保护系统提示词 + 标记为 preserve 的消息
/// 5. 派发异步摘要任务
pub struct ContextManager {
    /// 完整消息列表（包含已压缩的历史摘要）
    messages: Vec<ContextMessage>,
    /// 压缩策略
    strategy: ContextStrategy,
    /// Token 估算器
    tokenizer: TokenEstimator,
    /// 统计信息
    stats: ContextStats,
    /// 当前估算的 Token 总数（缓存，增量更新）
    cached_token_count: usize,
    /// 异步摘要任务的 sender（可选）
    summary_tx: Option<tokio::sync::mpsc::UnboundedSender<SummaryTask>>,
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
    /// 被保留的重要消息数
    pub preserved_count: usize,
}

impl ContextManager {
    /// 创建新的上下文管理器
    pub fn new(system_prompt: String, strategy: ContextStrategy) -> Self { ... }

    /// 添加消息（自动触发压缩检查）
    /// 返回是否触发了压缩
    pub fn add_message(&mut self, message: ChatMessage) -> bool { ... }

    /// 获取当前消息列表（发送给模型前的最终视图）
    pub fn get_messages(&self) -> Vec<ChatMessage> { ... }

    /// 手动触发压缩
    pub fn compress(&mut self) -> CompressResult { ... }

    /// 获取统计信息
    pub fn stats(&self) -> &ContextStats { ... }

    /// 切换压缩策略
    pub fn set_strategy(&mut self, strategy: ContextStrategy) { ... }

    /// 将最后一条消息标记为永久保留
    /// 由 main.rs 在检测到"读取文件"等关键操作后调用
    pub fn preserve_last_message(&mut self) { ... }

    /// 请求异步摘要（返回后不阻塞，摘要完成后自动注入）
    pub fn request_async_summary(&mut self) { ... }
}
```

### 3.3 Token 估算器 (`tokenizer.rs`)

#### 3.3.1 增量式 Token 估算

> **设计变更**：原始方案每次全量遍历 O(n)，改为增量累加 O(1)。每次只估算新加的消息，加到缓存上。

```rust
/// 轻量级 Token 估算器
///
/// 估算规则（与原始方案一致）：
/// - 英文: ~4 chars/token
/// - 中文: ~1.5 chars/token
/// - 代码: ~3 chars/token
/// - 特殊字符: ~0.5 token/char
///
/// 精度目标: ±20%（需用 tiktoken 交叉验证校准）
pub struct TokenEstimator {
    /// 校准系数（可根据实际误差动态调整）
    calibration_factor: f64,
    /// 是否已校准
    calibrated: bool,
}

impl TokenEstimator {
    pub fn new() -> Self {
        TokenEstimator {
            calibration_factor: 1.0,
            calibrated: false,
        }
    }

    /// 估算文本的 Token 数
    pub fn estimate_text(&self, text: &str) -> usize {
        let mut tokens = 0f64;
        let mut ascii_count = 0usize;
        let mut cjk_count = 0usize;

        for ch in text.chars() {
            match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' => ascii_count += 1,
                '\u{4e00}'..='\u{9fff}' | '\u{3000}'..='\u{303f}' => cjk_count += 1,
                ' ' | '\n' | '\t' => tokens += 0.25,
                _ => tokens += 0.5,
            }
        }

        tokens += ascii_count as f64 / 4.0;
        tokens += cjk_count as f64 / 1.5;

        ((tokens * self.calibration_factor).ceil() as usize).max(1)
    }

    /// 估算单条消息的 Token（用于增量更新）
    pub fn estimate_message(&self, msg: &ChatMessage) -> usize {
        // ... 与原始方案一致
    }

    /// ⭐ 校准接口：用 tiktoken 跑一批真实数据，计算校准系数
    ///
    /// ```ignore
    /// // 校准脚本（独立于主程序运行）:
    /// // 1. 收集 100 条真实对话消息
    /// // 2. 用本估算器计算总 Token: estimated
    /// // 3. 用 tiktoken-rs 计算总 Token: actual
    /// // 4. calibration_factor = actual / estimated
    /// ```
    pub fn calibrate(&mut self, estimated: usize, actual: usize) {
        if estimated > 0 && actual > 0 {
            self.calibration_factor = actual as f64 / estimated as f64;
            self.calibrated = true;
        }
    }
}
```

#### 3.3.2 精度校准方案（新增）

在仓库中提供独立的校准脚本 `scripts/calibrate_tokenizer.py`，用于定期验证估算精度：

```python
#!/usr/bin/env python3
"""
Token 估算器校准脚本

用法: 收集一批真实对话消息 (JSON 格式)，
      用 tiktoken 计算精确 Token 数，
      与本方案的经验公式做对比，输出校准系数。

输出示例:
    messages: 100
    estimated: 45,230
    actual:    52,100 (cl100k_base)
    error:     +13.2%
    calibration_factor: 1.152
"""
import json, tiktoken

enc = tiktoken.get_encoding("cl100k_base")
# ... 加载消息，计算，输出对比
```

将此校准作为 CI 门禁或月度任务，确保估算精度不退化。

---

### 3.4 ⭐ 压缩策略实现 (`strategy.rs`) — 重设计

#### 3.4.1 核心设计原则

原始方案中，Summarization 在"即将超限时同步触发"，有**逻辑自相矛盾**的问题：

```
❌ 原始设计的死锁:
  1. Token 达到 70% → 触发压缩
  2. 滑动窗口不够 → 需要 LLM 做摘要
  3. 调用 LLM 做摘要 → 额外消耗 Token
  4. 摘要结果塞回 → 可能仍然超限
```

```
✅ 新设计的分层策略:
  层1: 滑动窗口（同步，O(1)，永远不会失败）
  层2: 异步摘要（后台，低优先级，空闲时执行）
  层3: 保底截断（同步，极端情况下使用）
```

#### 3.4.2 三层压缩模型

```rust
/// 压缩结果
pub enum CompressResult {
    /// 无需压缩
    NotNeeded,
    /// 已通过滑动窗口压缩
    SlidingWindowCompressed {
        removed_count: usize,
        removed_turns: usize,
        summary: String,
    },
    /// ⭐ 已触发异步摘要任务（摘要完成后会自动注入）
    AsyncSummaryDispatched {
        task_id: u64,
    },
    /// ⭐ 保底截断（所有方法都无效时的最后手段）
    HardTruncated {
        removed_count: usize,
        kept_count: usize,
    },
}

impl ContextStrategy {
    /// ⭐ 主入口：三层压缩
    ///
    /// 层1: 滑动窗口（总是可以执行，保证不会崩溃）
    /// 层2: 异步摘要（后台执行，不阻塞主循环）
    /// 层3: 保底截断（极端情况下的安全网）
    pub fn compress(
        &self,
        messages: &mut Vec<ContextMessage>,
        stats: &mut ContextStats,
        summary_tx: Option<&tokio::sync::mpsc::UnboundedSender<SummaryTask>>,
    ) -> CompressResult {
        match self {
            ContextStrategy::SlidingWindow { max_turns } => {
                Self::sliding_window_compress(messages, *max_turns, stats)
            }
            ContextStrategy::Auto { token_limit, max_turns, trigger_ratio, enable_async_summary } => {
                Self::auto_compress(messages, *token_limit, *max_turns, *trigger_ratio, *enable_async_summary, stats, summary_tx)
            }
        }
    }
}
```

#### 3.4.3 层1：滑动窗口（同步，永不失败）

```rust
/// ⭐ 滑动窗口压缩（保留重要消息）
///
/// 保留规则（按优先级）：
/// 1. 系统提示词 → 始终保留
/// 2. preserved = true 的消息 → 永久保留
/// 3. importance = Milestone 的消息 → 尽量保留
/// 4. 最近 N 轮对话 → 保留
///
/// 丢弃顺序（按优先级最低的先丢）：
/// 1. 最早的非重要普通消息
/// 2. 最早的重要消息（如果 preserve 标记未设置）
/// 3. 永不丢弃系统提示词和 preserved 消息
fn sliding_window_compress(
    messages: &mut Vec<ContextMessage>,
    max_turns: usize,
    stats: &mut ContextStats,
) -> CompressResult {
    let original_len = messages.len();

    // 分离"不可丢弃"和"可丢弃"的消息
    let protected_indices: Vec<usize> = messages.iter()
        .enumerate()
        .filter(|(_, m)| {
            matches!(&m.message, ChatMessage::System { .. }) || m.preserved
        })
        .map(|(i, _)| i)
        .collect();

    // 计算可丢弃区域（系统消息之后，保留消息之外的区域）
    let turns = Self::count_turns(messages);
    if turns <= max_turns {
        return CompressResult::NotNeeded;
    }
    let remove_turns = turns - max_turns;

    // 找到要移除的消息范围（跳过 protected 消息）
    let mut removed_count = 0;
    let mut turns_removed = 0;
    let mut i = protected_indices.last().unwrap_or(&0) + 1;

    while i < messages.len() && turns_removed < remove_turns {
        if messages[i].preserved {
            i += 1;
            continue;
        }
        // 检查是否为一轮的结束
        if let ChatMessage::Assistant { tool_calls, .. } = &messages[i].message {
            if tool_calls.is_empty() {
                turns_removed += 1;
            }
        }
        // 移除消息
        messages.remove(i);
        removed_count += 1;
        // 不增加 i，因为 remove 后后续元素前移了
    }

    if removed_count > 0 {
        stats.compressed = true;
        stats.last_compressed_at = Some(std::time::Instant::now());

        return CompressResult::SlidingWindowCompressed {
            removed_count,
            removed_turns: remove_turns,
            summary: format!(
                "已移除最早 {} 轮对话（{} 条消息），保留 {} 条重要消息",
                remove_turns,
                removed_count,
                protected_indices.len(),
            ),
        };
    }

    CompressResult::NotNeeded
}
```

#### 3.4.4 层2：异步摘要（后台执行，不阻塞主循环）

```rust
/// ⭐ 异步摘要的设计思路：
///
/// 不在"即将超限"时救火，而是在对话空闲期做预处理。
///
/// 触发时机（不是等到 70%）：
/// - 对话轮数 > 10 轮且用户停顿 > 3 秒（空闲触发）
/// - 每次滑动窗口压缩后（压缩后的剩余消息进入摘要队列）
/// - 用户显式调用 /summarize 命令
///
/// 执行方式：
/// - 通过 mpsc channel 派发任务给后台 tokio task
/// - 摘要完成后自动将摘要消息注入 messages
/// - 摘要消息本身标记为 preserved，防止被再次压缩
fn auto_compress(
    messages: &mut Vec<ContextMessage>,
    token_limit: usize,
    max_turns: usize,
    enable_async_summary: bool,
    stats: &mut ContextStats,
    summary_tx: Option<&tokio::sync::mpsc::UnboundedSender<SummaryTask>>,
) -> CompressResult {
    let current_tokens = Self::estimate_total_tokens(messages);
    let trigger_threshold = (token_limit as f64 * trigger_ratio) as usize;

    stats.estimated_tokens = current_tokens;
    stats.usage_ratio = current_tokens as f64 / token_limit as f64;

    // 情况 A: Token 远低于阈值 → 无需操作
    if current_tokens < trigger_threshold {
        // 但如果轮数较多，可以"预先"派发摘要任务（空闲时压缩）
        let turns = Self::count_turns(messages);
        if enable_async_summary && turns > max_turns / 2 && summary_tx.is_some() {
            // 非阻塞派发，让后台慢慢做摘要
            let _ = summary_tx.unwrap().send(SummaryTask {
                messages: messages.clone(),
                // 只摘要非 preserved 的早期消息
                scope: SummaryScope::EarlyNonPreserved { keep_recent: max_turns },
            });
            // 返回 NotNeeded，摘要完成后后台自动注入
        }
        return CompressResult::NotNeeded;
    }

    // 情况 B: Token 超过阈值 → 先执行滑动窗口（一定成功）
    let turns = Self::count_turns(messages);
    if turns > max_turns {
        let result = Self::sliding_window_compress(messages, max_turns, stats);
        if matches!(&result, CompressResult::SlidingWindowCompressed { .. }) {
            // 滑动窗口后，如果还有盈余，可以顺便派发摘要
            if enable_async_summary && summary_tx.is_some() {
                let _ = summary_tx.unwrap().send(SummaryTask {
                    messages: messages.clone(),
                    scope: SummaryScope::AllNonPreserved,
                });
            }
            return result;
        }
    }

    // 情况 C: 滑动窗口后仍然超限（极端情况）→ 保底截断
    if current_tokens > token_limit {
        return Self::hard_truncate(messages, token_limit, stats);
    }

    CompressResult::NotNeeded
}
```

#### 3.4.5 层3：保底截断（最后的安全网）

```rust
/// ⭐ 保底截断：极端情况下的最后手段
///
/// 触发条件：滑动窗口执行后 Token 仍然超过硬限制
/// 执行策略：从最早的非保护消息开始截断，直到 Token 低于安全线
///
/// 注意：这会导致信息丢失，但总比 API 返回 400 好
fn hard_truncate(
    messages: &mut Vec<ContextMessage>,
    token_limit: usize,
    stats: &mut ContextStats,
) -> CompressResult {
    let original_len = messages.len();

    // 保护系统提示词和 preserved 消息
    let protected_count = messages.iter()
        .filter(|m| matches!(&m.message, ChatMessage::System { .. }) || m.preserved)
        .count();

    // 从最早的非保护消息开始删除
    let mut i = protected_count;
    while i < messages.len() {
        let current_tokens = Self::estimate_total_tokens(messages);
        if current_tokens < token_limit {
            break;
        }
        if !messages[i].preserved {
            messages.remove(i);
            // 删除后不增加 i
        } else {
            i += 1;
        }
    }

    let removed_count = original_len - messages.len();
    stats.compressed = true;

    CompressResult::HardTruncated {
        removed_count,
        kept_count: messages.len(),
    }
}
```

---

### 3.5 异步摘要生成器 (`summarizer.rs`) — 新增

```rust
/// 异步摘要任务
#[derive(Debug, Clone)]
pub struct SummaryTask {
    pub messages: Vec<ContextMessage>,
    pub scope: SummaryScope,
}

#[derive(Debug, Clone)]
pub enum SummaryScope {
    /// 摘要所有非 preserved 的早期消息，保留最近 N 轮
    EarlyNonPreserved { keep_recent: usize },
    /// 摘要所有非 preserved 消息
    AllNonPreserved,
}

/// 异步摘要生成器
///
/// 运行在独立的 tokio task 中，通过 channel 接收任务。
/// 摘要完成后，将摘要消息通过回调注入 ContextManager。
pub struct AsyncSummarizer {
    /// 接收摘要任务的 channel
    task_rx: tokio::sync::mpsc::UnboundedReceiver<SummaryTask>,
    /// 摘要完成后的回调（将摘要注入 ContextManager）
    on_complete: tokio::sync::mpsc::UnboundedSender<ContextMessage>,
}

impl AsyncSummarizer {
    /// 启动后台摘要任务
    pub fn start(
        model_adapter: Arc<dyn ModelAdapter>,
    ) -> (tokio::sync::mpsc::UnboundedSender<SummaryTask>, JoinHandle<()>) {
        let (task_tx, task_rx) = tokio::sync::mpsc::unbounded_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        // 后台任务
        let handle = tokio::spawn(async move {
            let summarizer = AsyncSummarizer { task_rx, on_complete: result_tx };
            summarizer.run(model_adapter).await;
        });

        (task_tx, handle)
    }

    async fn run(&mut self, model_adapter: Arc<dyn ModelAdapter>) {
        while let Some(task) = self.task_rx.recv().await {
            // 1. 提取需要摘要的消息
            let to_summarize = self.select_messages(&task);

            if to_summarize.is_empty() {
                continue;
            }

            // 2. 生成摘要（调用轻量模型或 LLM）
            let summary = match &model_adapter {
                // 如果有轻量模型（如本地小模型），用轻量模型做摘要
                // 否则用当前模型做摘要（但控制输出长度）
                _ => self.generate_llm_summary(&model_adapter, &to_summarize).await,
            };

            if let Ok(summary_text) = summary {
                // 3. 构建摘要消息
                let summary_message = ContextMessage {
                    message: ChatMessage::user(format!(
                        "【历史对话摘要 - 由系统自动生成】\n{}",
                        summary_text
                    )),
                    preserved: true,    // 摘要本身标记为永久保留
                    importance: MessageImportance::Important,
                };

                // 4. 发送回主线程
                let _ = self.on_complete.send(summary_message);
            }
        }
    }

    /// ⭐ 结构化摘要生成（比纯拼接更智能）
    async fn generate_llm_summary(
        &self,
        model: &dyn ModelAdapter,
        messages: &[ContextMessage],
    ) -> anyhow::Result<String> {
        let summary_prompt = ChatMessage::user(format!(
            r#"请将以下对话压缩为结构化摘要，保留关键决策和技术细节。

要求：
1. 按「目标 → 已执行操作 → 关键发现 → 当前状态」组织
2. 保留所有文件路径、命令、错误信息
3. 控制在 300 字以内
4. 如果涉及技术方案选择，标注"【决策】"

对话内容：
{}

请直接输出摘要，不要额外解释。"#,
            format_messages_for_summary(messages)
        ));

        let mut stream = model.stream_chat(
            &[ChatMessage::system("你是一个精准的结构化摘要助手。输出简洁，保留可操作信息。")],
            json!([]),
        );

        let mut summary = String::new();
        while let Some(event) = stream.next().await {
            if let ModelEvent::Text(text) = event {
                summary.push_str(&text);
            }
        }

        Ok(summary)
    }
}
```

#### 3.5.1 规则摘要（轻量级兜底，无需 LLM 调用）

保留原始方案的规则摘要作为**兜底方案**——当异步摘要任务队列积压或 LLM 不可用时使用：

```rust
/// 规则摘要（非 LLM 版本，用于异步摘要不可用时的兜底）
///
/// ⭐ 改进：结构化输出，保留决策链路，而不仅仅是拼接关键词
fn rule_based_summary(messages: &[ContextMessage]) -> String {
    let mut sections = SummarySections::new();

    for ctx_msg in messages {
        match &ctx_msg.message {
            ChatMessage::User { content } => {
                sections.add_user_intent(content);
            }
            ChatMessage::Tool { content, .. } => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                    extract_tool_info(&val, &mut sections);
                }
            }
            ChatMessage::Assistant { content, .. } => {
                sections.add_assistant_response(content);
                // 检测决策语句
                if content.contains("决定") || content.contains("选择") || content.contains("改为") {
                    sections.add_decision(content);
                }
            }
            _ => {}
        }
    }

    format!(
        "【历史对话摘要】\n\
         ── 用户意图 ──\n{}\n\n\
         ── 已执行操作 ──\n{}\n\n\
         ── 关键决策 ──\n{}\n\n\
         ── 当前状态 ──\n{}",
        sections.user_intents.join("\n"),
        sections.executed_ops.join("\n"),
        sections.decisions.join("\n"),
        sections.current_state()
    )
}

struct SummarySections {
    user_intents: Vec<String>,
    executed_ops: Vec<String>,
    decisions: Vec<String>,
    key_files: Vec<String>,
}

impl SummarySections {
    fn add_user_intent(&mut self, content: &str) {
        let intent: String = content.chars().take(80).collect();
        if !self.user_intents.contains(&intent) {
            self.user_intents.push(intent);
        }
    }

    fn add_decision(&mut self, content: &str) {
        // 提取决策相关语句（包含"决定/选择/改为"的句子）
        for sentence in content.split('。') {
            if sentence.contains("决定") || sentence.contains("选择") || sentence.contains("改为") {
                self.decisions.push(sentence.trim().to_string());
            }
        }
    }

    fn current_state(&self) -> String {
        if self.key_files.is_empty() {
            "无文件变更".to_string()
        } else {
            format!("已修改文件: {}", self.key_files.join(", "))
        }
    }
}
```

---

### 3.6 系统集成（与 main.rs 的集成）

```rust
// ⭐ 优化后的 main.rs 集成

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
            enable_async_summary: true,
        },
    );

    // 启动异步摘要后台任务
    let (summary_tx, summary_handle) = if let ContextStrategy::Auto { enable_async_summary: true, .. } = ctx.strategy() {
        let (tx, handle) = AsyncSummarizer::start(query_client.clone());
        (Some(tx), Some(handle))
    } else {
        (None, None)
    };
    ctx.set_summary_channel(summary_tx.clone());

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
            // ⭐ 使用 eprint! 输出到 stderr，避免被 Shell 工具捕获
            eprint!(
                "\r\x1b[2K[Token: {}/{} ({:.0}%) | 保留 {} 条重要消息] ",
                TokenEstimator::format_tokens(stats.estimated_tokens),
                TokenEstimator::format_tokens(128_000),
                stats.usage_ratio * 100.0,
                stats.preserved_count,
            );
        }

        // 检查是否有异步摘要结果需要注入
        if let Some(ref mut rx) = ctx.summary_result_rx {
            while let Ok(summary_msg) = rx.try_recv() {
                ctx.inject_summary(summary_msg);
                // ⭐ 使用 eprint! 通知用户
                eprintln!("\r\x1b[2K📋 异步摘要已生成并注入上下文");
            }
        }

        let mut stream_chat = query_client.stream_chat(
            ctx.get_messages(),
            tool_manager.get_tools_scehma(),
        );

        // ... 后续保持不变
    }
}
```

#### 3.6.1 系统提示词补充

```rust
// 系统提示词中追加上下文管理说明
// ⭐ 区分 stdout 和 stderr 的解释

let system_prompt = format!(
    r#"你当前工作的目录为 {}。

{}  // 权限摘要

【上下文管理说明】
- 为了管理上下文窗口，早期对话历史可能会被自动压缩为摘要。
- 摘要会按「目标 → 操作 → 决策 → 状态」的结构保留关键信息。
- 如果发现某些上下文缺失，请基于摘要信息继续工作。
- 重要的上下文信息请**写入文件**，而不是仅依赖对话历史。
- 系统状态信息（如 Token 使用率）会输出到 stderr，不会混入你的工具执行结果。

【工作原则】
- 读取文件内容后，关键信息应记录在文件中，不要仅依赖对话记忆。
- 如果需要在多轮对话中保持状态，请使用文件持久化。"#,
    current_dir,
    policy_summary,
);
```

---

### 3.7 ⭐ 终端输出规范：stdout vs stderr 分离

这是原始方案中遗漏的重要问题：**Token 状态信息如果输出到 stdout，会被 Shell 工具捕获，模型下次执行命令时会看到这些日志，造成困惑**。

```rust
// ✅ 正确做法：
// - 用户交互（">" 提示符、模型输出）→ stdout
// - 系统状态（Token 使用率、压缩通知、摘要完成通知）→ stderr

// 终端显示效果（stderr 输出，不影响 stdout 的纯净性）：
//
// stderr: [Token: 23.5K/128K (18%) | 保留 2 条重要消息]
// stdout: > 帮我修改 src/main.rs
// stdout: 我来查看一下文件内容...
// stderr: ─── 🔧 调用工具: shell ───
// stderr:   $ cat src/main.rs
// stdout: (cat 命令的输出)
```

---

## 4. 实现计划

```
Phase 1: Token 估算器（0.25天）
├── 实现 TokenEstimator（基于字符统计的经验公式）
├── 实现增量估算（每次只估新增消息，缓存总计数）
├── 单元测试：英文、中文、代码、混合文本
└── 校准脚本：scripts/calibrate_tokenizer.py

Phase 2: 消息重要性标记系统（0.25天）
├── 实现 ContextMessage + MessageImportance
├── 实现自动分类（auto_classify）
├── 实现 preserve 标记接口
└── 单元测试：重要性分类正确性

Phase 3: 三层压缩策略（0.5天）
├── 实现滑动窗口（preserve 消息保护逻辑）
├── 实现自动模式（三层触发逻辑）
├── 实现保底截断
├── 实现对话轮数计数器（考虑 preserve 消息）
└── 单元测试：边界条件、保留正确性

Phase 4: 异步摘要生成器（0.25天）
├── 实现 AsyncSummarizer（channel + background task）
├── 实现结构化 LLM 摘要提示词
├── 实现规则摘要兜底
└── 单元测试：摘要格式、异步注入

Phase 5: ContextManager + 系统集成（0.25天）
├── 实现 ContextManager（增量 Token + 自动压缩触发）
├── 实现统计信息收集 (ContextStats + preserved_count)
├── 修改 main.rs 使用 ContextManager
├── stdout/stderr 分离
├── 系统提示词补充
└── 集成测试：长时间对话不崩溃

总计: 1.5 天
```

---

## 5. 测试策略

### 5.1 单元测试

| 测试用例 | 目标 |
|---------|------|
| `test_estimate_short_text` | 短文本 Token 估算 |
| `test_estimate_long_code` | 代码文本 Token 估算 |
| `test_estimate_message_types` | 各消息类型 Token 估算 |
| `test_incremental_token_tracking` | 增量估算与全量估算结果一致 |
| `test_sliding_window_basic` | 基本滑动窗口功能 |
| `test_sliding_window_protects_system` | 系统提示词不被删除 |
| `test_sliding_window_protects_preserved` | ⭐ 标记为 preserve 的消息不被删除 |
| `test_sliding_window_milestone_priority` | ⭐ Milestone 消息优先保留 |
| `test_sliding_window_exact_limit` | 恰好等于窗口大小的边界 |
| `test_sliding_window_below_limit` | 低于窗口大小不触发 |
| `test_auto_no_compression_needed` | 低 Token 使用不触发 |
| `test_auto_sliding_window_first` | ⭐ 自动模式优先滑动窗口 |
| `test_auto_hard_truncate` | ⭐ 极端情况下保底截断 |
| `test_context_manager_add_and_compress` | 完整添加+压缩流程 |
| `test_context_manager_stats` | 统计信息正确性（含 preserved_count） |
| `test_importance_classification` | ⭐ 自动分类正确性 |
| `test_rule_based_summary_structure` | ⭐ 规则摘要结构化输出 |

### 5.2 集成测试

```rust
#[tokio::test]
async fn test_long_conversation_with_preserved_messages() {
    let mut ctx = ContextManager::new(
        "system prompt".to_string(),
        ContextStrategy::Auto {
            token_limit: 10_000,    // 小限制方便测试
            max_turns: 3,
            trigger_ratio: 0.5,
            enable_async_summary: false,  // 测试中关闭异步摘要
        },
    );

    // 模拟 50 轮对话，其中一些标记为重要
    for i in 0..50 {
        ctx.add_message(ChatMessage::user(format!("user message {}", i)));

        // 模拟工具调用
        ctx.add_message(ChatMessage::assistant_tool_calls(
            format!("thinking {}", i),
            vec![ToolCall { id: format!("call_{}", i), name: "shell".into(), arguments: r#"{"command": "echo ok"}"#.into() }],
        ));

        let tool_result = ChatMessage::tool(
            format!("call_{}", i),
            r#"{"ok": true, "result": {"stdout": "ok\n"}}"#.into(),
        );
        ctx.add_message(tool_result);

        ctx.add_message(ChatMessage::assistant(format!("response {}", i)));

        // 第 10 轮标记为重要（模拟读取了关键文件）
        if i == 10 {
            ctx.preserve_last_message();
        }

        // 验证系统提示词始终存在
        assert!(ctx.get_messages().iter().any(|m| matches!(m, ChatMessage::System { .. })));
    }

    // 验证系统提示词还在
    assert!(ctx.get_messages().iter().any(|m| matches!(m, ChatMessage::System { .. })));

    // 验证压缩后的消息数不会无限增长
    assert!(ctx.get_messages().len() < 50, "消息数应被压缩控制");

    // ⭐ 验证被 preserve 的消息没有被丢弃
    let stats = ctx.stats();
    assert!(stats.preserved_count > 0, "应有被保留的重要消息");
    assert!(stats.compressed, "应已触发压缩");
}
```

### 5.3 精度验证测试

```rust
/// ⭐ Token 估算精度交叉验证
///
/// 需要 tiktoken-rs 作为 dev-dependency 运行
#[cfg(feature = "calibration")]
#[tokio::test]
async fn test_token_estimator_accuracy() {
    use tiktoken_rs::cl100k_base;

    let estimator = TokenEstimator::new();
    let bpe = cl100k_base().unwrap();

    // 从 fixtures 加载真实对话样本
    let samples = load_test_fixtures("tests/fixtures/real_conversations.json");

    let mut total_estimated = 0;
    let mut total_actual = 0;

    for sample in &samples {
        let estimated = estimator.estimate_text(sample);
        let actual = bpe.encode_with_special_tokens(sample).len();
        total_estimated += estimated;
        total_actual += actual;
    }

    let error = (total_estimated as f64 - total_actual as f64) / total_actual as f64;
    let error_pct = error * 100.0;

    println!("Total estimated: {}", total_estimated);
    println!("Total actual:    {}", total_actual);
    println!("Error:           {:.1}%", error_pct);

    // 允许 ±20% 误差
    assert!(
        error.abs() < 0.20,
        "Token 估算误差 {:.1}% 超出 ±20% 允许范围",
        error_pct
    );

    // 计算校准系数
    let calibration_factor = total_actual as f64 / total_estimated as f64;
    println!("Recommended calibration_factor: {:.3}", calibration_factor);
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
异步摘要需要 `tokio` 的 `sync` 和 `spawn` 能力（项目应该已经有了）。

### 6.2 终端显示效果

```
# stderr 输出（Token 信息、系统通知）：
[Token: 23.5K/128K (18%) | 保留 2 条重要消息]

# stdout 输出（用户交互、模型输出）：
> 帮我修改 src/main.rs
我来查看一下文件内容...

# stderr 输出（工具调用通知）：
━━━ 🔧 调用工具: shell ───
  $ cat src/main.rs

# stdout 输出（命令执行结果）：
fn main() { ... }

# stderr 输出（压缩通知）：
📋 滑动窗口压缩: 已移除最早 3 轮对话，节省 ~12K tokens
📋 异步摘要已生成并注入上下文

# 注意：上述 stderr 输出不会被 Shell 工具捕获，
# 模型执行 "cat src/main.rs" 时不会看到 Token 信息
```

---

## 7. 风险评估

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|---------|
| Token 估算不精确导致过早/过晚压缩 | 中 | 高 | ⭐ 校准脚本定期跑，动态调整 calibration_factor；触发阈值留余量（70% 触发） |
| 异步摘要延迟导致压缩不及时 | 中 | 中 | ⭐ 滑动窗口先执行（同步），摘要只是锦上添花；保底截断兜底 |
| preserve 标记滥用导致上下文膨胀 | 中 | 中 | ⭐ 设置 preserve 上限（最多 10 条）；超过上限按重要性降级 |
| 重要消息被误判为普通消息而丢弃 | 中 | 中 | ⭐ auto_classify 保守策略（宁多勿少）；用户可通过命令显式标记 |
| 滑动窗口切断了工具调用的完整上下文 | 低 | 低 | 按"轮"切割，不会切断一轮中间的消息 |
| 性能影响（每次添加消息都做分类） | 低 | 低 | 分类逻辑 O(1)，< 0.1ms |
| stdout/stderr 混杂导致 Shell 输出异常 | 低 | 低 | ⭐ 严格执行 stdout/stderr 分离原则 |

---

## 8. 后续扩展方向

| 阶段 | 功能 | 说明 |
|------|------|------|
| V1 | 滑动窗口 + Token 显示 + 重要性标记 | 本方案核心内容 |
| V2 | 异步 LLM 摘要 | 后台结构化摘要生成 |
| V3 | 持久化压缩索引 | 压缩的内容存入文件，模型可"翻页"查看 |
| V4 | 自适应窗口 | 根据当前对话复杂度动态调整窗口大小 |
| V5 | 结构化上下文 | 将文件内容、工具输出、对话分开展示给模型 |
| V6 | 语义去重 | 检测并移除重复的文件读取结果 |

---

## 9. 为什么这是 P0

1. **系统稳定性**：不做上下文管理，对话超过 10-15 轮必然崩溃，系统**不可用**
2. **成本控制**：每次 API 调用都发送全部历史，Token 消耗随对话轮次线性增长
3. **用户体验**：Token 消耗透明化（终端显示），用户可以感知上下文状态
4. **与权限沙箱互补**：权限沙箱让模型"安全地做事"，上下文管理让模型"持续地做事"

---

## 附录：原始方案 vs 优化方案对照

| 维度 | 原始方案 | 优化方案 |
|------|---------|---------|
| **摘要触发时机** | 即将超限时同步触发（矛盾） | 空闲时异步预处理（合理） |
| **消息保留策略** | 一律平等，先入先出 | ⭐ 重要性分级，关键信息永久保留 |
| **Token 估算** | 每次全量 O(n) | ⭐ 增量累加 O(1) |
| **压缩通知输出** | stdout | ⭐ stderr（避免被 Shell 捕获） |
| **摘要格式** | 关键词拼接 | ⭐ 结构化：目标→操作→决策→状态 |
| **极端情况** | 无兜底 | ⭐ 三层模型：滑动窗口→异步摘要→保底截断 |
| **估算精度** | 无校准机制 | ⭐ 校准脚本 + calibration_factor 动态调整 |
| **依赖** | 无需新增 | 无需新增（异步摘要复用已有 ModelAdapter） |

---

> **设计原则**: 上下文窗口管理应该做到「透明、自动、无损」。
> **核心哲学**: 不是所有消息都平等——让有限窗口内保留最有价值的信息，而不是最新的信息。
> **工程哲学**: 永远不要阻塞主循环——压缩操作必须 O(1) 或异步执行。
