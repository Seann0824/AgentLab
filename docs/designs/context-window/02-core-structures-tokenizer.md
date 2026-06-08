# 功能特性设计：上下文窗口管理 (Context Window Management) — 核心结构与 Token 估算

> 原文拆分自 `../context-window.md`。


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

