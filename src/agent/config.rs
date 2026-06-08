use crate::context::ContextStrategy;
use crate::swarm::registry::AgentType;

/// Agent 配置：上下文策略和运行参数
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// 上下文 Token 上限
    pub token_limit: usize,
    /// 最大轮次（触发滑动窗口的轮数）
    pub max_turns: usize,
    /// 压缩触发比例（0.0 ~ 1.0）
    pub trigger_ratio: f64,
    /// 是否启用异步摘要
    pub enable_async_summary: bool,
    /// 是否启用工具调用修剪
    pub enable_tool_pruning: bool,
    /// 保留最近工具调用数
    pub tool_pruning_keep_recent: usize,
    /// 工具输出最大字符数（超过的被截断）
    pub tool_pruning_max_output_chars: usize,
    /// Agent 类型（用于渲染不同的身份提示词）
    pub agent_type: AgentType,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,
            enable_async_summary: true,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
            agent_type: AgentType::Orchestrator,
        }
    }
}

impl AgentConfig {
    /// 将配置转换为 ContextStrategy
    pub fn to_strategy(&self) -> ContextStrategy {
        ContextStrategy::Auto {
            token_limit: self.token_limit,
            max_turns: self.max_turns,
            trigger_ratio: self.trigger_ratio,
            enable_async_summary: self.enable_async_summary,
            enable_tool_pruning: self.enable_tool_pruning,
            tool_pruning_keep_recent: self.tool_pruning_keep_recent,
            tool_pruning_max_output_chars: self.tool_pruning_max_output_chars,
        }
    }
}
