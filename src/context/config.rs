use serde::{Deserialize, Serialize};

/// 上下文压缩策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextStrategy {
    /// 滑动窗口：只保留最近 N 轮对话
    SlidingWindow {
        /// 保留的最大轮数（用户+助手算一轮）
        max_turns: usize,
    },
    /// 自动模式：四层渐进压缩
    ///
    /// 层级（从轻到重）：
    /// 0. 工具调用结果修剪（占位符替换，保留对话结构）
    /// 1. 滑动窗口（删除整轮对话）
    /// 2. 异步摘要（LLM 生成摘要，大幅压缩）
    /// 3. 保底截断（极端情况，删除非保护消息）
    Auto {
        /// Token 硬限制（模型上下文窗口大小）
        token_limit: usize,
        /// 滑动窗口保留轮数（压缩后的保底）
        max_turns: usize,
        /// 触发压缩的 Token 阈值（占限制的百分比 0.0~1.0）
        trigger_ratio: f64,
        /// 是否启用异步摘要（默认启用）
        enable_async_summary: bool,
        /// ⭐ 是否启用工具调用修剪（层0，默认启用）
        enable_tool_pruning: bool,
        /// ⭐ 工具修剪时保留的最近轮次数（最近的 N 轮工具结果不动）
        tool_pruning_keep_recent: usize,
        /// ⭐ 工具修剪时，工具输出保留的最大字符数（超出部分替换为占位符）
        tool_pruning_max_output_chars: usize,
    },
}

impl Default for ContextStrategy {
    fn default() -> Self {
        ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,
            enable_async_summary: true,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
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

    /// 是否启用工具调用修剪
    pub fn tool_pruning_enabled(&self) -> bool {
        match self {
            ContextStrategy::Auto {
                enable_tool_pruning,
                ..
            } => *enable_tool_pruning,
            ContextStrategy::SlidingWindow { .. } => false,
        }
    }

    /// 获取工具修剪的 keep_recent 参数
    pub fn tool_pruning_keep_recent(&self) -> usize {
        match self {
            ContextStrategy::Auto {
                tool_pruning_keep_recent,
                ..
            } => *tool_pruning_keep_recent,
            ContextStrategy::SlidingWindow { .. } => 0,
        }
    }

    /// 获取工具修剪的 max_output_chars 参数
    pub fn tool_pruning_max_output_chars(&self) -> usize {
        match self {
            ContextStrategy::Auto {
                tool_pruning_max_output_chars,
                ..
            } => *tool_pruning_max_output_chars,
            ContextStrategy::SlidingWindow { .. } => usize::MAX,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_strategy() {
        let strategy = ContextStrategy::default();
        assert!(strategy.tool_pruning_enabled());
        assert_eq!(strategy.tool_pruning_keep_recent(), 3);
        assert_eq!(strategy.tool_pruning_max_output_chars(), 200);
    }

    #[test]
    fn test_sliding_window_no_pruning() {
        let strategy = ContextStrategy::SlidingWindow { max_turns: 5 };
        assert!(!strategy.tool_pruning_enabled());
        assert_eq!(strategy.max_turns(), 5);
    }

    #[test]
    fn test_auto_max_turns() {
        let strategy = ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 10,
            trigger_ratio: 0.7,
            enable_async_summary: true,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        };
        assert_eq!(strategy.max_turns(), 10);
        assert_eq!(strategy.token_limit(), Some(128_000));
    }
}
