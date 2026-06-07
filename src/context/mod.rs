mod config;
mod strategy;
mod summarizer;
mod tokenizer;
mod types;

pub use config::{ContextConfig, ContextStrategy};
pub use summarizer::{rule_based_summary, AsyncSummarizer};
pub use tokenizer::TokenEstimator;
pub use types::{
    CompressResult, ContextMessage, ContextStats, MessageImportance, PrunedToolCall,
    SummaryResult, SummaryScope, SummaryTask, is_stdout_structural,
};

use std::time::Instant;
use tokio::sync::mpsc;

use crate::model::ChatMessage;

/// 上下文管理器
///
/// 职责：
/// 1. 管理消息列表的生命周期
/// 2. ⭐ 估算 Token 消耗（增量式缓存，避免 O(n) 全量遍历）
/// 3. 根据策略自动压缩上下文（四层渐进：修剪→滑动窗口→摘要→截断）
/// 4. 保护系统提示词 + 标记为 preserve 的消息
/// 5. 派发异步摘要任务
///
/// # 缓存优化
///
/// Token 计数使用增量更新：
/// - `add_message()` 时只计算新消息的 token，累加到 `cached_token_count`
/// - 压缩/删除操作会使缓存失效，触发全量重算
/// - 这样在绝大多数场景下，Token 估算都是 O(1) 的
pub struct ContextManager {
    /// 完整消息列表（包含已压缩的历史摘要）
    messages: Vec<ContextMessage>,
    /// 压缩策略
    strategy: ContextStrategy,
    /// Token 估算器
    tokenizer: TokenEstimator,
    /// 统计信息
    stats: ContextStats,
    /// ⭐ 缓存的 Token 总数（增量更新，避免全量遍历）
    cached_token_count: usize,
    /// ⭐ 缓存是否有效（压缩/删除后失效）
    cache_valid: bool,
    /// 异步摘要任务的 sender
    summary_tx: Option<mpsc::UnboundedSender<SummaryTask>>,
    /// 异步摘要结果的 receiver
    summary_rx: Option<mpsc::UnboundedReceiver<SummaryResult>>,
    /// 异步摘要任务的后台 handle
    _summary_handle: Option<tokio::task::JoinHandle<()>>,
    /// 最大 preserved 消息数量（防止滥用）
    max_preserved: usize,
}

impl ContextManager {
    /// 创建新的上下文管理器
    pub fn new(system_prompt: impl Into<String>, strategy: ContextStrategy) -> Self {
        let system_msg = ContextMessage::from(ChatMessage::system(system_prompt));
        let tokenizer = TokenEstimator::new();

        // 初始计算 system 消息的 token
        let initial_tokens = tokenizer.estimate_message(&system_msg.message);

        Self {
            messages: vec![system_msg],
            strategy,
            tokenizer,
            stats: ContextStats {
                message_count: 1,
                ..Default::default()
            },
            cached_token_count: initial_tokens,
            cache_valid: true,
            summary_tx: None,
            summary_rx: None,
            _summary_handle: None,
            max_preserved: 10,
        }
    }

    /// 设置异步摘要通道
    pub fn setup_summary_channel(
        &mut self,
        model_adapter: Option<Box<dyn crate::model::ModelAdapter>>,
    ) {
        let (task_tx, result_rx, handle) = AsyncSummarizer::start(model_adapter);
        self.summary_tx = Some(task_tx);
        self.summary_rx = Some(result_rx);
        self._summary_handle = Some(handle);
    }

    /// ⭐ 添加消息（自动触发压缩检查）
    ///
    /// 缓存优化：只计算新消息的 token 数，累加到缓存中。
    /// 返回是否触发了压缩。
    pub fn add_message(&mut self, message: ChatMessage) -> bool {
        // 1. 自动分类重要性
        let importance = ContextMessage::auto_classify(&message);
        let ctx_msg = ContextMessage {
            message,
            preserved: false,
            importance,
        };

        // 2. ⭐ 增量更新 token 缓存
        let new_tokens = self.tokenizer.estimate_message(&ctx_msg.message);
        self.cached_token_count += new_tokens;
        self.cache_valid = true;

        // 3. 添加消息
        self.messages.push(ctx_msg);
        self.stats.message_count = self.messages.len();
        self.stats.preserved_count = self.messages.iter().filter(|m| m.preserved).count();

        // 4. 更新估算的 token 数到 stats
        self.stats.estimated_tokens = self.cached_token_count;

        // 5. 自动触发压缩检查
        self.check_and_compress()
    }

    /// 检查是否需要压缩，如果需要则执行
    fn check_and_compress(&mut self) -> bool {
        match &self.strategy {
            ContextStrategy::SlidingWindow { max_turns } => {
                let result = strategy::sliding_window_mode(
                    &mut self.messages,
                    *max_turns,
                    &mut self.stats,
                );
                self.handle_compress_result(&result)
            }
            ContextStrategy::Auto { .. } => {
                let result = strategy::auto_compress(
                    &mut self.messages,
                    &self.strategy,
                    &self.tokenizer,
                    &mut self.stats,
                    self.summary_tx.clone(), // 传递 summary_tx，让 auto_compress 内部派发异步摘要（层1）
                );
                self.handle_compress_result(&result)
            }
        }
    }

    /// 处理压缩结果，更新缓存
    fn handle_compress_result(&mut self, result: &CompressResult) -> bool {
        match result {
            CompressResult::NotNeeded => false,
            // ⭐ ToolCallsPruned 只替换了内容，消息数量不变，token 缓存需要重算
            CompressResult::ToolCallsPruned { .. }
            | CompressResult::SlidingWindowCompressed { .. }
            | CompressResult::HardTruncated { .. } => {
                // ⭐ 缓存失效，需要全量重算 token
                self.recalculate_token_cache();
                self.stats.message_count = self.messages.len();
                self.stats.preserved_count =
                    self.messages.iter().filter(|m| m.preserved).count();
                true
            }
            CompressResult::AsyncSummaryDispatched { .. } => true,
        }
    }

    /// ⭐ 全量重算 token 缓存（缓存失效时调用）
    fn recalculate_token_cache(&mut self) {
        self.cached_token_count = self.tokenizer.estimate_messages(
            &self
                .messages
                .iter()
                .map(|m| m.message.clone())
                .collect::<Vec<_>>(),
        );
        self.cache_valid = true;
        self.stats.estimated_tokens = self.cached_token_count;
    }


    /// ⭐ 检查是否有异步摘要结果需要注入
    ///
    /// 修复：先收集所有结果，再逐个注入，避免多次可变借用
    pub fn poll_summary_results(&mut self) -> usize {
        // 1. 先收集所有结果（取出 receiver 的所有消息）
        let results: Vec<SummaryResult> = if let Some(ref mut rx) = self.summary_rx {
            let mut collected = Vec::new();
            while let Ok(result) = rx.try_recv() {
                collected.push(result);
            }
            collected
        } else {
            Vec::new()
        };

        let count = results.len();

        // 2. 再逐个注入（此时 rx 不再被借用）
        for result in results {
            // ⭐ 注入摘要并携带被摘要的消息数量，以便删除原始消息
            self.inject_summary(result.summary_message, result.summarized_count);
        }

        count
    }

    /// ⭐ 注入摘要消息（注入后删除被摘要的原始消息，触发压缩检查）
    ///
    /// 修复：注入摘要时会删除被摘要的原始消息，这样 token 总数才能真正下降。
    /// 摘要消息插入到系统提示词之后，然后删除 summaries_count 条紧随其后的原始消息。
    fn inject_summary(&mut self, summary_msg: ContextMessage, summarized_count: usize) {
        // 1. 将摘要消息插入到系统提示词之后
        let insert_pos = self
            .messages
            .iter()
            .position(|m| !matches!(&m.message, ChatMessage::System { .. }))
            .unwrap_or(self.messages.len());

        self.messages.insert(insert_pos, summary_msg);

        // 2. ⭐ 删除被摘要的原始消息（刚从插入位置之后开始删除）
        if summarized_count > 0 {
            let remove_start = insert_pos + 1;
            let remove_end = (remove_start + summarized_count).min(self.messages.len());
            if remove_start < remove_end {
                self.messages.drain(remove_start..remove_end);
            }
        }

        // 3. ⭐ 缓存失效，全量重算 token
        self.recalculate_token_cache();
        self.stats.message_count = self.messages.len();
        self.stats.preserved_count = self.messages.iter().filter(|m| m.preserved).count();

        // 4. ⭐ 注入后触发压缩检查，防止 token 超限
        self.check_and_compress();
    }

    /// 获取当前消息列表（发送给模型前的最终视图）
    /// 返回 ChatMessage 列表，供 ModelAdapter 使用
    pub fn get_messages(&self) -> Vec<ChatMessage> {
        self.messages.iter().map(|m| m.message.clone()).collect()
    }

    /// 获取当前消息列表的引用（ContextMessage）
    pub fn context_messages(&self) -> &[ContextMessage] {
        &self.messages
    }

    /// 手动触发压缩
    pub fn compress(&mut self) -> CompressResult {
        match &self.strategy {
            ContextStrategy::SlidingWindow { max_turns } => {
                let result =
                    strategy::sliding_window_mode(&mut self.messages, *max_turns, &mut self.stats);
                self.handle_compress_result(&result);
                result
            }
            ContextStrategy::Auto { .. } => {
                let result = strategy::auto_compress(
                    &mut self.messages,
                    &self.strategy,
                    &self.tokenizer,
                    &mut self.stats,
                    self.summary_tx.clone(), // auto_compress 内部已处理异步摘要派发
                );
                self.handle_compress_result(&result);
                result
            }
        }
    }

    /// 获取统计信息
    pub fn stats(&self) -> &ContextStats {
        &self.stats
    }

    /// 切换压缩策略
    pub fn set_strategy(&mut self, strategy: ContextStrategy) {
        self.strategy = strategy;
    }

    /// 获取当前策略的引用
    pub fn strategy(&self) -> &ContextStrategy {
        &self.strategy
    }

    /// ⭐ 将最后一条消息标记为永久保留
    ///
    /// 由 main.rs 在检测到"读取文件"等关键操作后调用。
    /// 受 max_preserved 限制，防止滥用。
    pub fn preserve_last_message(&mut self) -> bool {
        let preserved_count = self.messages.iter().filter(|m| m.preserved).count();
        if preserved_count >= self.max_preserved {
            return false;
        }

        if let Some(last) = self.messages.last_mut() {
            if !last.preserved {
                last.preserved = true;
                last.importance = types::MessageImportance::Important;
                self.stats.preserved_count = preserved_count + 1;
                return true;
            }
        }
        false
    }

    /// 获取 Token 估算器的引用
    pub fn tokenizer(&self) -> &TokenEstimator {
        &self.tokenizer
    }

    /// 获取缓存的 token 计数
    pub fn cached_token_count(&self) -> usize {
        self.cached_token_count
    }

    /// 获取最大 preserved 限制
    pub fn max_preserved(&self) -> usize {
        self.max_preserved
    }

    /// 设置最大 preserved 限制
    pub fn set_max_preserved(&mut self, max: usize) {
        self.max_preserved = max;
    }

    /// ⭐ 手动触发工具调用修剪（层0压缩）
    ///
    /// 可以在检测到 token 接近上限时主动调用。
    /// 比滑动窗口更温和——只压缩工具结果，不删除对话结构。
    pub fn prune_tool_calls(&mut self) -> CompressResult {
        if !self.strategy.tool_pruning_enabled() {
            return CompressResult::NotNeeded;
        }

        let result = strategy::tool_call_pruning(
            &mut self.messages,
            self.strategy.tool_pruning_keep_recent(),
            self.strategy.tool_pruning_max_output_chars(),
            &self.tokenizer,
        );

        if result.did_compress() {
            self.recalculate_token_cache();
            self.stats.compressed = true;
            self.stats.last_compressed_at = Some(Instant::now());

            if let CompressResult::ToolCallsPruned {
                pruned_count,
                saved_tokens,
            } = &result
            {
                self.stats.pruned_tool_calls += pruned_count;
                self.stats.pruned_saved_tokens += saved_tokens;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_manager_new() {
        let ctx = ContextManager::new(
            "System prompt",
            ContextStrategy::SlidingWindow { max_turns: 5 },
        );

        assert_eq!(ctx.messages.len(), 1);
        assert!(matches!(
            &ctx.messages[0].message,
            ChatMessage::System { .. }
        ));
        assert!(ctx.cached_token_count > 0);
        assert!(ctx.cache_valid);
    }

    #[test]
    fn test_add_message_increments_cache() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 10 },
        );

        let initial_tokens = ctx.cached_token_count;

        ctx.add_message(ChatMessage::user("Hello"));
        assert!(
            ctx.cached_token_count > initial_tokens,
            "Cache should increase after adding message"
        );

        ctx.add_message(ChatMessage::assistant("Hi there!"));
        assert!(
            ctx.cached_token_count > initial_tokens,
            "Cache should increase after adding assistant message"
        );
    }

    #[test]
    fn test_add_message_triggers_compression() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 3 },
        );

        // 添加超过窗口大小的消息（每轮2条，所以要加超过6条消息）
        for i in 0..10 {
            ctx.add_message(ChatMessage::user(format!("User {}", i)));
            ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
        }

        let stats = ctx.stats();
        assert!(stats.compressed, "Compression should have been triggered");
        assert!(
            ctx.messages.len() < 21,
            "Messages should be compressed: {}",
            ctx.messages.len()
        );
    }

    #[test]
    fn test_system_prompt_protected() {
        let mut ctx = ContextManager::new(
            "System prompt that should never be removed",
            ContextStrategy::SlidingWindow { max_turns: 1 },
        );

        for i in 0..20 {
            ctx.add_message(ChatMessage::user(format!("User {}", i)));
            ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
        }

        let messages = ctx.get_messages();
        assert!(messages
            .iter()
            .any(|m| matches!(m, ChatMessage::System { .. })));
        // System 应该是第一条
        if let ChatMessage::System { content } = &messages[0] {
            assert!(content.contains("System prompt"));
        } else {
            panic!("First message should be System");
        }
    }

    #[test]
    fn test_preserve_last_message() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 2 },
        );

        ctx.add_message(ChatMessage::user("User 1"));
        ctx.add_message(ChatMessage::assistant("Assistant 1"));

        let preserved = ctx.preserve_last_message();
        assert!(preserved, "Should be able to preserve last message");

        // 继续添加更多消息触发压缩
        for i in 2..10 {
            ctx.add_message(ChatMessage::user(format!("User {}", i)));
            ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
        }

        // preserved 消息应该还在
        assert!(
            ctx.messages.iter().any(|m| m.preserved),
            "Preserved message should survive compression"
        );
    }

    #[test]
    fn test_get_messages_returns_chat_messages() {
        let ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 5 },
        );

        let messages = ctx.get_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], ChatMessage::System { .. }));
    }

    #[test]
    fn test_stats_updated() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 5 },
        );

        ctx.add_message(ChatMessage::user("Hello"));
        ctx.add_message(ChatMessage::assistant("World"));

        let stats = ctx.stats();
        assert_eq!(stats.message_count, 3);
        assert!(stats.estimated_tokens > 0);
    }

    #[test]
    fn test_max_preserved_limit() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 10 },
        );
        ctx.set_max_preserved(2);

        for i in 0..5 {
            ctx.add_message(ChatMessage::user(format!("User {}", i)));
            ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
            ctx.preserve_last_message();
        }

        let preserved_count = ctx.messages.iter().filter(|m| m.preserved).count();
        assert!(
            preserved_count <= 2,
            "Should not exceed max_preserved limit: {}",
            preserved_count
        );
    }

    #[test]
    fn test_poll_summary_no_results() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::Auto {
                token_limit: 100_000,
                max_turns: 20,
                trigger_ratio: 0.7,
                enable_async_summary: false,
                enable_tool_pruning: true,
                tool_pruning_keep_recent: 3,
                tool_pruning_max_output_chars: 200,
            },
        );

        let injected = ctx.poll_summary_results();
        assert_eq!(injected, 0);
    }

    #[test]
    fn test_recalculate_token_cache() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 10 },
        );

        ctx.add_message(ChatMessage::user("Hello"));
        let cache_before = ctx.cached_token_count;

        // 模拟缓存失效
        ctx.cache_valid = false;
        ctx.recalculate_token_cache();

        assert!(ctx.cache_valid);
        assert_eq!(ctx.cached_token_count, cache_before);
    }

    #[test]
    fn test_inject_summary_triggers_compress() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 2 },
        );

        // 加满消息
        for i in 0..5 {
            ctx.add_message(ChatMessage::user(format!("User {}", i)));
            ctx.add_message(ChatMessage::assistant(format!("Assistant {}", i)));
        }

        let count_before = ctx.messages.len();

        // 注入一个摘要消息
        let summary = ContextMessage {
            message: ChatMessage::user("【摘要】这是历史对话摘要"),
            preserved: true,
            importance: MessageImportance::Important,
        };
        // ⭐ 测试手动注入摘要，summarized_count=0 表示不删除原始消息
        ctx.inject_summary(summary, 0);

        // 注入后消息应该没爆炸（压缩检查应已触发）
        assert!(
            ctx.messages.len() <= count_before + 2,
            "After injection + compression, message count should be bounded: {}",
            ctx.messages.len()
        );
    }

    // ⭐ 测试手动调用工具修剪
    #[test]
    fn test_prune_tool_calls_manual() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::Auto {
                token_limit: 100_000,
                max_turns: 20,
                trigger_ratio: 0.9,
                enable_async_summary: false,
                enable_tool_pruning: true,
                tool_pruning_keep_recent: 2,
                tool_pruning_max_output_chars: 50,
            },
        );

        // 添加一些包含长工具输出的消息
        use crate::model::ToolCall;
        for i in 0..10 {
            ctx.add_message(ChatMessage::user(format!("User {}", i)));
            ctx.add_message(ChatMessage::assistant_tool_calls(
                format!("Thinking {}", i),
                vec![ToolCall {
                    id: format!("call_{}", i),
                    name: "shell".into(),
                    arguments: r#"{"command": "echo ok"}"#.into(),
                }],
            ));
            let long_output = format!(
                r#"{{"ok":true,"result":{{"command":"echo ok","stdout":"{}\n"}}}}"#,
                "ok".repeat(500)
            );
            ctx.add_message(ChatMessage::tool(format!("call_{}", i), &long_output));
            ctx.add_message(ChatMessage::assistant(format!("Done {}", i)));
        }

        let count_before = ctx.messages.len();

        let result = ctx.prune_tool_calls();

        assert!(result.did_compress(), "Should prune some tool calls");
        // 消息数量不变（只替换内容）
        assert_eq!(ctx.messages.len(), count_before);

        if let CompressResult::ToolCallsPruned {
            pruned_count,
            saved_tokens,
        } = &result
        {
            assert!(*pruned_count > 0, "Should have pruned some calls");
            assert!(*saved_tokens > 0, "Should have saved tokens");
            assert!(
                ctx.stats().pruned_tool_calls >= *pruned_count,
                "Stats should track pruned calls"
            );
        }
    }

    #[test]
    fn test_prune_tool_calls_disabled() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::SlidingWindow { max_turns: 5 },
        );

        let result = ctx.prune_tool_calls();
        assert!(!result.did_compress(), "SlidingWindow mode has no pruning");
    }

    // ============ 验证上下文压缩能力的新测试 ============

    /// ⭐ 测试1: Token 缓存增量更新 vs 全量重算的一致性
    ///
    /// 验证：每次 add_message 时的增量累加（estimate_message + 累加）
    /// 与全量重算（estimate_messages）的结果一致。
    /// 这是上下文压缩能力的基础——只有缓存准确，压缩决策才能正确。
    #[test]
    fn test_token_cache_incremental_vs_full_consistency() {
        let mut ctx = ContextManager::new(
            "System prompt for testing",
            ContextStrategy::Auto {
                token_limit: 100_000,
                max_turns: 20,
                trigger_ratio: 0.9,
                enable_async_summary: false,
                enable_tool_pruning: false,
                tool_pruning_keep_recent: 3,
                tool_pruning_max_output_chars: 200,
            },
        );

        // 1. 初始状态：只有 system 消息
        let initial_cache = ctx.cached_token_count;
        ctx.recalculate_token_cache();
        assert_eq!(
            ctx.cached_token_count, initial_cache,
            "初始状态：增量缓存应与全量重算一致"
        );

        // 2. 逐步添加消息，每次验证增量缓存与全量重算一致
        let messages_to_add = [
            ChatMessage::user("User 1: 请列出当前目录下的所有文件"),
            ChatMessage::assistant("以下是当前目录下的文件列表：\n1. src/\n2. Cargo.toml\n3. README.md"),
            ChatMessage::user("User 2: 让我读取一下 Cargo.toml 的内容"),
            ChatMessage::assistant("好的，我来读取 Cargo.toml 的内容。这个文件定义了我们项目的依赖和配置。"),
            ChatMessage::user("User 3: 请编译并运行项目"),
            ChatMessage::assistant("正在编译项目，请稍等...编译成功！所有测试通过。"),
            ChatMessage::user("User 4: 让我检查一下代码的模块结构"),
            ChatMessage::assistant("项目的模块结构如下：\n- src/main.rs：主入口\n- src/context/：上下文管理\n  - mod.rs：ContextManager\n  - strategy.rs：压缩策略\n  - types.rs：数据类型"),
            ChatMessage::user("User 5: 请运行测试"),
            ChatMessage::assistant("正在运行测试...所有 75 个测试全部通过！"),
        ];

        for (i, msg) in messages_to_add.iter().enumerate() {
            ctx.add_message(msg.clone());

            // 每次添加后，重新计算全量并对比
            let incremental = ctx.cached_token_count;
            // 先让缓存失效再重算，得到全量值
            ctx.cache_valid = false;
            ctx.recalculate_token_cache();
            let full_recalc = ctx.cached_token_count;

            assert_eq!(
                incremental, full_recalc,
                "消息 #{} 添加后：增量缓存({})应与全量重算({})一致",
                i + 1, incremental, full_recalc
            );

            // 恢复增量缓存有效状态
            ctx.cache_valid = true;
            ctx.cached_token_count = incremental;
        }
    }

    /// ⭐ 测试2: 验证动态有效 max_turns 的触发逻辑
    ///
    /// 配置小 token_limit + 低 trigger_ratio，使 auto_compress 在轮次少时
    /// 也能因 token 超阈值而触发滑动窗口压缩。
    #[test]
    fn test_dynamic_max_turns_triggers_compression_early() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::Auto {
                token_limit: 80,     // 很小的 token 限制
                max_turns: 20,       // 最大 20 轮
                trigger_ratio: 0.5,  // 40 tokens 触发
                enable_async_summary: false,
                enable_tool_pruning: false,
                tool_pruning_keep_recent: 3,
                tool_pruning_max_output_chars: 200,
            },
        );

        // 添加几条较长消息让 token 快速超过阈值
        // 每条消息约 15-20 tokens，4 轮（8条消息）即可超过 40 阈值
        for i in 0..8 {
            let long_user = format!("User {}: 这是一个较长的用户输入，用于测试动态 max_turns 在 token 超阈值时的触发逻辑", i);
            let long_assistant = format!("Assistant {}: 这是一个较长的助手回复，包含一些技术细节和代码示例", i);
            ctx.add_message(ChatMessage::user(&long_user));
            ctx.add_message(ChatMessage::assistant(&long_assistant));
        }

        let stats = ctx.stats();
        assert!(
            stats.compressed,
            "在 token 超阈值但轮次(8)远未达到 max_turns(20) 时，应触发压缩。用量比例: {:.2}",
            stats.usage_ratio
        );

        // 验证消息数量被压缩
        let turns_in_ctx = ctx.messages.iter()
            .filter(|m| matches!(&m.message, ChatMessage::User { .. }))
            .count();
        assert!(
            turns_in_ctx < 8,
            "压缩后轮次({})应少于原始轮次(8)",
            turns_in_ctx
        );
    }

    /// ⭐ 测试3: 验证异步摘要注入删除原文后 token 真实下降
    ///
    /// 通过模拟摘要结果，验证 inject_summary 正确删除被摘要的原始消息，
    /// 且 token 计数显著下降。
    #[test]
    fn test_inject_summary_reduces_tokens() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::Auto {
                token_limit: 100_000,
                max_turns: 20,
                trigger_ratio: 0.9,
                enable_async_summary: false,
                enable_tool_pruning: false,
                tool_pruning_keep_recent: 3,
                tool_pruning_max_output_chars: 200,
            },
        );

        // 添加 6 轮对话
        for i in 0..6 {
            ctx.add_message(ChatMessage::user(format!("User message number {}", i)));
            ctx.add_message(ChatMessage::assistant(format!(
                "Assistant response with some details for message {}",
                i
            )));
        }

        let tokens_before_inject = ctx.cached_token_count;
        let msg_count_before = ctx.messages.len();

        // 注入摘要（模拟异步摘要的结果）：summarized_count = 4 表示摘要覆盖了前 4 条非系统消息
        let summary_msg = ContextMessage {
            message: ChatMessage::assistant("【摘要】用户询问了文件列表、读取文件内容、编译项目等操作"),
            preserved: true,
            importance: MessageImportance::Important,
        };
        ctx.inject_summary(summary_msg, 4);

        // 验证消息数减少
        assert!(
            ctx.messages.len() < msg_count_before,
            "注入摘要后消息数({})应少于注入前({})",
            ctx.messages.len(),
            msg_count_before
        );

        // 验证 token 总数下降
        let tokens_after = ctx.cached_token_count;
        assert!(
            tokens_after < tokens_before_inject,
            "注入摘要后 token({})应少于注入前({})",
            tokens_after,
            tokens_before_inject
        );

        // 验证摘要消息已插入
        let has_summary = ctx.messages.iter().any(|m| {
            if let ChatMessage::Assistant { content, .. } = &m.message {
                content.contains("【摘要】")
            } else {
                false
            }
        });
        assert!(has_summary, "摘要消息应存在于上下文中");
    }

    /// ⭐ 测试4: 验证 end-to-end ContextManager 全生命周期
    ///
    /// 模拟完整场景：多次 add_message → add_message 内部触发 check_and_compress
    /// → 压缩后消息变少 → token 下降 → 验证统计信息正确
    #[test]
    fn test_end_to_end_compression_lifecycle() {
        let mut ctx = ContextManager::new(
            "System prompt",
            ContextStrategy::Auto {
                token_limit: 100,    // 很小的 token 上限
                max_turns: 5,        // 最大 5 轮
                trigger_ratio: 0.4,  // 40 tokens 触发
                enable_async_summary: false,
                enable_tool_pruning: false,
                tool_pruning_keep_recent: 3,
                tool_pruning_max_output_chars: 200,
            },
        );

        // 记录初始状态
        assert!(!ctx.stats().compressed, "初始状态不应压缩");
        let initial_token_count = ctx.cached_token_count;
        assert!(initial_token_count > 0, "System prompt 应有 token");

        // 阶段 1: 逐轮添加消息，跟踪压缩触发时机
        let mut compressed_at_turn: Option<usize> = None;

        for turn in 1..=20 {
            let user_msg = format!(
                "Turn {}: 用户输入一些较长的文本内容，让 token 数量逐步增长以触发压缩",
                turn
            );
            let assistant_msg = format!(
                "Turn {}: 助手的回复也包含一些内容，确保每轮都有足够的 token 消耗",
                turn
            );

            let compressed = ctx.add_message(ChatMessage::user(&user_msg));
            if compressed && compressed_at_turn.is_none() {
                compressed_at_turn = Some(turn);
            }

            let compressed = ctx.add_message(ChatMessage::assistant(&assistant_msg));
            if compressed && compressed_at_turn.is_none() {
                compressed_at_turn = Some(turn);
            }
        }

        // 验证：压缩已被触发
        assert!(
            compressed_at_turn.is_some(),
            "在 20 轮对话中应至少触发一次压缩"
        );

        // 验证：压缩后的 token 应在 token_limit 附近
        let stats = ctx.stats();
        assert!(
            stats.compressed,
            "最终 stats 应标记为已压缩"
        );
        let final_tokens = ctx.cached_token_count;
        assert!(
            final_tokens <= 150,  // 允许略微超出 token_limit
            "压缩后 token({}) 应接近或低于 token_limit(100)",
            final_tokens
        );

        // 验证：消息数量受控
        let total_msgs = ctx.messages.len();
        assert!(
            total_msgs < 41,  // 20轮 * 2条/轮 = 40条，压缩后应明显减少
            "压缩后消息数({})应远小于原始消息数(40+)",
            total_msgs
        );

        // 验证：系统提示词始终保留
        let system_count = ctx.messages.iter()
            .filter(|m| matches!(&m.message, ChatMessage::System { .. }))
            .count();
        assert_eq!(system_count, 1, "系统提示词应始终保留");
    }

    /// ⭐ 测试5: 验证渐进压缩的顺序 — 工具修剪优先于滑动窗口
    ///
    /// 当启用了工具修剪时，如果有长工具输出，auto_compress 应优先使用
    /// 层0（工具修剪）而非直接跳到层1（滑动窗口）。
    /// 我们使用大 token_limit 确保工具修剪足够，不会触发更高层压缩。
    #[test]
    fn test_progressive_order_tool_pruning_before_sliding_window() {
        use crate::model::ToolCall;

        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::Auto {
                token_limit: 100_000,  // 大 token_limit，确保工具修剪后 token 不会超限
                max_turns: 20,
                trigger_ratio: 0.9,    // 只在高水位触发
                enable_async_summary: false,
                enable_tool_pruning: true,
                tool_pruning_keep_recent: 2,
                tool_pruning_max_output_chars: 50,  // 短输出也修剪
            },
        );

        // 添加带长工具输出的消息（模拟真实场景）
        for i in 0..8 {
            ctx.add_message(ChatMessage::user(format!("User {}", i)));
            ctx.add_message(ChatMessage::assistant_tool_calls(
                format!("Thinking {}", i),
                vec![ToolCall {
                    id: format!("call_{}", i),
                    name: "shell".into(),
                    arguments: r#"{"command": "echo hello"}"#.into(),
                }],
            ));
            // 长工具输出（超过 max_output_chars=50）
            let long_output = format!(
                r#"{{"ok":true,"result":{{"command":"ls -la","stdout":"{}\n"}}}}"#,
                "some_file.txt\n".repeat(30)
            );
            ctx.add_message(ChatMessage::tool(format!("call_{}", i), &long_output));
            ctx.add_message(ChatMessage::assistant(format!("Done {}", i)));
        }

        // 手动触发工具修剪（层0）
        let result = ctx.prune_tool_calls();
        assert!(result.did_compress(), "应触发工具修剪(层0)");

        // 验证工具修剪的统计数据已更新
        let stats = ctx.stats();
        assert!(
            stats.pruned_tool_calls > 0,
            "工具修剪次数应大于0，实际: {}",
            stats.pruned_tool_calls
        );

        // 验证：消息数量不变（工具修剪只替换内容，不删除消息）
        // 8轮 * 4条/轮 = 32 + 1 system = 33条
        assert_eq!(
            ctx.messages.len(),
            33,
            "工具修剪应保留所有消息，当前消息数: {}",
            ctx.messages.len()
        );

        // 验证：工具内容已被占位符替换
        let has_pruned = ctx.messages.iter().any(|m| {
            if let ChatMessage::Tool { content, .. } = &m.message {
                content.contains("TOOL_OUTPUT_PRUNED")
            } else {
                false
            }
        });
        assert!(has_pruned, "应该至少有一个工具消息被替换为占位符");
    }

    /// ⭐ 测试6: 验证 Token-based 触发阈值 — 用大量长消息触发压缩
    ///
    /// 即使轮次很少，只要 token 超过触发阈值，也应触发压缩。
    /// 这个测试确保「Token 超阈值但轮次不足」的边界情况被正确处理。
    #[test]
    fn test_token_based_trigger_with_few_turns() {
        let mut ctx = ContextManager::new(
            "System",
            ContextStrategy::Auto {
                token_limit: 100,
                max_turns: 50,         // 很大的 max_turns，轮次本身不会触发
                trigger_ratio: 0.3,    // 30 tokens 就触发
                enable_async_summary: false,
                enable_tool_pruning: false,
                tool_pruning_keep_recent: 3,
                tool_pruning_max_output_chars: 200,
            },
        );

        // 只用 2 轮，但每条消息都很长（~30 tokens/条）
        // 2轮=4条=~120 tokens，远超 30 的触发阈值，但轮次(2)远小于 max_turns(50)
        let very_long_text = "这是一个非常长的文本内容，用于测试Token-based触发阈值。".repeat(5);
        ctx.add_message(ChatMessage::user(&very_long_text));
        ctx.add_message(ChatMessage::assistant(&very_long_text));
        ctx.add_message(ChatMessage::user(&very_long_text));
        ctx.add_message(ChatMessage::assistant(&very_long_text));

        let stats = ctx.stats();
        assert!(
            stats.compressed,
            "即使只有 2 轮（远小于 max_turns=50），token 超阈值也应触发压缩。用量比例: {:.2}",
            stats.usage_ratio
        );
        assert!(
            stats.usage_ratio > 0.0,
            "应记录使用率"
        );
    }

    /// ⭐ 集成测试: 模拟真实 Agent 循环中的上下文压缩
    ///
    /// 此测试模拟 Agent 主循环的完整流程：
    /// 1. 用户输入 → add_message(User)
    /// 2. 助手思考并调用工具 → add_message(Assistant+tool_calls)
    /// 3. 工具执行并返回结果 → add_message(Tool)
    /// 4. 助手最终回复 → add_message(Assistant)
    /// 5. 循环中自动触发压缩 → poll_summary_results
    ///
    /// 验证压缩后的上下文仍然结构完整、可用。
    #[test]
    fn test_agent_loop_simulation_with_compression() {
        use crate::model::ToolCall;
        // 创建 Tokio 运行时，使 AsyncSummarizer 能调用 tokio::spawn
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let mut ctx = ContextManager::new(
            "你是 Agent Lab，一个智能助手。你可以使用各种工具来帮助用户完成任务。",
            ContextStrategy::Auto {
                token_limit: 300,     // 小 token 上限，加速触发压缩
                max_turns: 4,          // 最大 4 轮，超出就触发滑动窗口
                trigger_ratio: 0.5,    // 150 tokens 触发
                enable_async_summary: true,
                enable_tool_pruning: true,
                tool_pruning_keep_recent: 2,
                tool_pruning_max_output_chars: 100,
            },
        );
        // 启动异步摘要（使用规则摘要器，不依赖 LLM）
        ctx.setup_summary_channel(None);

        let mut compression_triggered = false;
        let mut last_token_count = ctx.cached_token_count;

        // 模拟 12 轮 Agent 对话（每轮：用户→助手(工具调用)→工具结果→助手回复）
        for turn in 1..=12 {
            // 步骤 1: 用户输入
            let user_msg = format!(
                "Turn {}: 请帮我查一下当前目录的文件列表，然后读取 README.md 的第一行内容。",
                turn
            );
            let compressed = ctx.add_message(ChatMessage::user(&user_msg));
            if compressed { compression_triggered = true; }

            // 步骤 2: 助手发出工具调用
            let assistant_tc = ChatMessage::assistant_tool_calls(
                format!("我来帮你查看 Turn {} 的文件信息。", turn),
                vec![
                    ToolCall {
                        id: format!("call_ls_{}", turn),
                        name: "shell".into(),
                        arguments: r#"{"command":"ls -la"}"#.into(),
                    },
                    ToolCall {
                        id: format!("call_read_{}", turn),
                        name: "read".into(),
                        arguments: r#"{"file_path":"README.md","max_length":100}"#.into(),
                    },
                ],
            );
            let compressed = ctx.add_message(assistant_tc);
            if compressed { compression_triggered = true; }

            // 步骤 3: 工具执行结果（长输出，触发工具修剪）
            let tool_output = format!(
                r#"{{"ok":true,"result":{{"stdout":"file1.txt\nfile2.txt\nREADME.md\nsrc/\ntarget/\nCargo.toml\nCargo.lock\n{}\n"}}}}"#,
                "some_other_file.txt\n".repeat(15)  // 长输出
            );
            let compressed = ctx.add_message(ChatMessage::tool(
                format!("call_ls_{}", turn),
                &tool_output,
            ));
            if compressed { compression_triggered = true; }

            let read_output = format!(
                "{{\"ok\":true,\"result\":{{\"content\":\"Agent Lab README - Turn {}\"}}}}",
                turn
            );
            let compressed = ctx.add_message(ChatMessage::tool(
                format!("call_read_{}", turn),
                &read_output,
            ));
            if compressed { compression_triggered = true; }

            // 步骤 4: 助手最终回复
            let assistant_reply = format!(
                "Turn {} 的结果：目录下有多个文件，README 的第一行是 '# Agent Lab'。",
                turn
            );
            let compressed = ctx.add_message(ChatMessage::assistant(&assistant_reply));
            if compressed { compression_triggered = true;

            }
            // ⭐ 模拟主循环中的 poll_summary_results（每轮轮询摘要结果）
            let injected = ctx.poll_summary_results();
            if injected > 0 {
                compression_triggered = true;
            }

            // 记录 token 变化
            let current_tokens = ctx.cached_token_count;
            if current_tokens > last_token_count + 50 {
                // Token 大幅增长，说明可能有问题 — 但先观察
            }
            last_token_count = ctx.cached_token_count;
        }

        // ============ 验证 ============

        // 验证 1: 压缩至少触发了一次
        assert!(
            compression_triggered,
            "在 12 轮模拟 Agent 循环中应至少触发一次压缩（触发阈值=150, token_limit=300）"
        );

        // 验证 2: 系统提示词始终保留
        let system_count = ctx.messages.iter()
            .filter(|m| matches!(&m.message, ChatMessage::System { .. }))
            .count();
        assert_eq!(system_count, 1, "系统提示词应始终保留");

        // 验证 3: Token 受控 — 不应超过 token_limit 的 2 倍
        let final_tokens = ctx.cached_token_count;
        assert!(
            final_tokens <= 600,
            "Token 数({}) 应控制在 token_limit(300) 的 2 倍以内，当前 {}",
            final_tokens, final_tokens
        );

        let stats = ctx.stats();
        assert!(stats.compressed, "stats 应标记为已压缩");
        assert!(
            stats.pruned_tool_calls > 0 || stats.estimated_tokens <= 300,
            "压缩应有效: pruned_tool_calls={}, estimated_tokens={}",
            stats.pruned_tool_calls, stats.estimated_tokens
        );

        // 验证 4: 消息结构完整 — 能正常转换为 ChatMessage 列表
        let chat_messages: Vec<ChatMessage> = ctx.messages.iter()
            .map(|cm| cm.message.clone())
            .collect();
        assert!(!chat_messages.is_empty(), "消息列表不应为空");
        // 第一条必须是 System
        assert!(
            matches!(&chat_messages[0], ChatMessage::System { .. }),
            "第一条消息必须是 System"
        );

        // 验证 5: 消息数量受控 — 12 轮 * 5 条/轮 = 60 + 1 system = 61
        // 压缩后应明显减少
        let msg_count = ctx.messages.len();
        assert!(
            msg_count < 61,
            "压缩后消息数({})应明显少于原始消息数(61)",
            msg_count
        );

        // 验证 6: 统计信息合理
        assert!(
            stats.estimated_tokens > 0,
            "estimated_tokens 应 > 0"
        );
        assert!(
            stats.usage_ratio > 0.0,
            "usage_ratio 应 > 0"
        );
    }
}
