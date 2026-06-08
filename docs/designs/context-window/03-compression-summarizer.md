# 功能特性设计：上下文窗口管理 (Context Window Management) — 压缩策略与异步摘要

> 原文拆分自 `../context-window.md`。

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

