mod config;
mod strategy;
mod summarizer;
mod tokenizer;
mod types;

pub use config::{ContextConfig, ContextStrategy};
pub use summarizer::{rule_based_summary, AsyncSummarizer};
pub use tokenizer::TokenEstimator;
pub use types::{
    CompressResult, ContextMessage, ContextStats, MessageImportance, SummaryResult, SummaryScope,
    SummaryTask, is_stdout_structural,
};

use tokio::sync::mpsc;

use crate::model::ChatMessage;

/// 上下文管理器
///
/// 职责：
/// 1. 管理消息列表的生命周期
/// 2. ⭐ 估算 Token 消耗（增量式缓存，避免 O(n) 全量遍历）
/// 3. 根据策略自动压缩上下文
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
                );
                let compressed = self.handle_compress_result(&result);

                // 如果启用了异步摘要且压缩发生，派发摘要任务
                if compressed {
                    self.maybe_dispatch_summary();
                }

                compressed
            }
        }
    }

    /// 处理压缩结果，更新缓存
    fn handle_compress_result(&mut self, result: &CompressResult) -> bool {
        match result {
            CompressResult::NotNeeded => false,
            CompressResult::SlidingWindowCompressed { .. }
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

    /// 派发异步摘要任务（如果已启用）
    fn maybe_dispatch_summary(&mut self) {
        if let Some(ref tx) = self.summary_tx {
            let max_turns = match &self.strategy {
                ContextStrategy::Auto { max_turns, .. } => *max_turns,
                ContextStrategy::SlidingWindow { max_turns } => *max_turns,
            };
            let task = SummaryTask {
                messages: self.messages.clone(),
                scope: SummaryScope::EarlyNonPreserved {
                    keep_recent: max_turns,
                },
            };
            let _ = tx.send(task);
        }
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
            self.inject_summary(result.summary_message);
        }

        count
    }

    /// ⭐ 注入摘要消息（注入后触发压缩检查）
    ///
    /// 注意：注入摘要可能使 token 总数增加，需要重新检查是否超限。
    fn inject_summary(&mut self, summary_msg: ContextMessage) {
        // 将摘要消息插入到系统提示词之后
        let insert_pos = self
            .messages
            .iter()
            .position(|m| !matches!(&m.message, ChatMessage::System { .. }))
            .unwrap_or(self.messages.len());

        // ⭐ 更新 token 缓存：加上摘要消息的 token
        let summary_tokens = self.tokenizer.estimate_message(&summary_msg.message);
        self.cached_token_count += summary_tokens;

        self.messages.insert(insert_pos, summary_msg);
        self.stats.message_count = self.messages.len();
        self.stats.preserved_count = self.messages.iter().filter(|m| m.preserved).count();

        // ⭐ 注入后触发压缩检查，防止 token 超限
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
                );
                self.handle_compress_result(&result);
                if matches!(&result, CompressResult::SlidingWindowCompressed { .. }) {
                    self.maybe_dispatch_summary();
                }
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
        ctx.inject_summary(summary);

        // 注入后消息应该没爆炸（压缩检查应已触发）
        assert!(
            ctx.messages.len() <= count_before + 2,
            "After injection + compression, message count should be bounded: {}",
            ctx.messages.len()
        );
    }
}
