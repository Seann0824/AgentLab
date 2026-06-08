mod config;
mod strategy;
mod summarizer;
mod tokenizer;
mod types;

pub use config::{ContextConfig, ContextStrategy};
pub use strategy::force_compress as force_compress_strategy;
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
    /// ⭐ 最近一次压缩结果（用于向用户展示压缩事件）
    last_compress_result: Option<CompressResult>,
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
            last_compress_result: None,
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
        // ⭐ 记录压缩结果（无论是否实际压缩，用于外部展示）
        self.last_compress_result = Some(result.clone());
        match result {
            CompressResult::NotNeeded => false,
            // ⭐ ToolCallsPruned 只替换了内容，消息数量不变，token 缓存需要重算
            CompressResult::ToolCallsPruned { .. }
            | CompressResult::SlidingWindowCompressed { .. }
            | CompressResult::HardTruncated { .. }
            | CompressResult::EmergencyTruncated { .. }
            | CompressResult::ForceCompressed { .. } => {
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
            // 🔴 清理孤立的 Tool 消息（按 count 删除可能破坏 tool_calls→Tool 对应关系）
            let orphaned = strategy::remove_orphaned_tool_messages(&mut self.messages);
            if orphaned > 0 {
                eprintln!(
                    "[inject_summary] 🧹 removed {} orphaned tool messages after summary injection",
                    orphaned,
                );
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

    /// ⭐ 获取并清除最后一次压缩结果（用于向用户展示压缩事件）
    pub fn take_last_compress_result(&mut self) -> Option<CompressResult> {
        self.last_compress_result.take()
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

    /// ⭐ 清空历史消息（保留系统提示词）
    ///
    /// 当用户输入 /clear 时调用，重置上下文到初始状态（仅保留 system prompt）。
    /// Token 缓存也会被重置。
    pub fn clear(&mut self) {
        // 只保留系统提示词（第一条消息）
        if let Some(system_msg) = self.messages.first().cloned() {
            // 确保 system 消息被标记为 preserved（避免被意外压缩）
            self.messages.clear();
            self.messages.push(system_msg);
        } else {
            // 理论上不会发生，但兜底
            self.messages.clear();
        }

        // 重置 token 缓存
        self.recalculate_token_cache();
        self.stats.message_count = self.messages.len();
        self.stats.preserved_count = self.messages.iter().filter(|m| m.preserved).count();
        self.stats.compressed = false;
        self.stats.pruned_tool_calls = 0;
        self.stats.pruned_saved_tokens = 0;
        self.stats.last_compressed_at = None;
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

    /// ⭐ 检查上下文是否阻塞（Token 使用率 >= 95%）
    /// 阻塞意味着需要强制压缩才能继续
    /// ⭐ 消费压缩标志（返回当前值并重置）
    ///
    /// 修复：`stats.compressed` 在压缩后会被设为 true，但永不重置。
    /// 这导致 agent loop 中每次迭代都认为"刚发生了压缩"，从而重复注入 goal/task/memory。
    /// 使用此方法消费后立即重置，确保只在压缩发生的那个迭代注入一次。
    pub fn consume_compressed_flag(&mut self) -> bool {
        let was_compressed = self.stats.compressed;
        self.stats.compressed = false;
        was_compressed
    }

    /// ⭐ 检查上下文是否阻塞（Token 使用率 >= 95%）
    pub fn is_blocked(&self) -> bool {
        self.stats.usage_ratio >= 0.95
    }

    /// ⭐ 检查上下文是否临界（Token 使用率 >= 90%）
    /// 临界意味着需要尽快触发压缩
    pub fn is_critical(&self) -> bool {
        self.stats.usage_ratio >= 0.90
    }

    /// ⭐ 强制压缩上下文（跳过 trigger_threshold 检查，直接执行最激进压缩）
    ///
    /// 在 is_blocked() 返回 true 时调用。
    /// 先发送异步摘要任务，然后执行同步压缩。
    pub fn force_compress(&mut self) -> CompressResult {
        eprintln!("[ContextManager] force_compress called (usage={:.0}%)", self.stats.usage_ratio * 100.0);
        
        let result = crate::context::force_compress_strategy(
            &mut self.messages,
            &self.strategy,
            &self.tokenizer,
            &mut self.stats,
            self.summary_tx.clone(),
        );

        if result.did_compress() {
            self.recalculate_token_cache();
            self.stats.message_count = self.messages.len();
            self.stats.preserved_count = self.messages.iter().filter(|m| m.preserved).count();
            self.last_compress_result = Some(result.clone());
        }

        result
    }
}

#[cfg(test)]
mod tests;
