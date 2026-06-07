use serde::{Deserialize, Serialize};

/// 上下文压缩策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextStrategy {
    /// 滑动窗口：只保留最近 N 轮对话
    SlidingWindow {
        /// 保留的最大轮数（用户+助手算一轮）
        max_turns: usize,
    },
    /// 自动模式：滑动窗口兜底 + 异步摘要压缩
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
        ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,
            enable_async_summary: true,
        }
    }
}

impl ContextStrategy {
    /// 获取 Token 硬限制（仅 Auto 模式有效）
    pub fn token_limit(&self) -> Option<usize> {
        match self {
            ContextStrategy::Auto { token_limit, .. } => Some(*token_limit),
            ContextStrategy::SlidingWindow { .. } => None,
        }
    }

    /// 获取滑动窗口保留轮数
    pub fn max_turns(&self) -> usize {
        match self {
            ContextStrategy::SlidingWindow { max_turns } => *max_turns,
            ContextStrategy::Auto { max_turns, .. } => *max_turns,
        }
    }
}

/// 上下文管理器的顶层配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// 压缩策略
    pub strategy: ContextStrategy,
    /// 系统提示词
    pub system_prompt: String,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            strategy: ContextStrategy::default(),
            system_prompt: String::new(),
        }
    }
}
